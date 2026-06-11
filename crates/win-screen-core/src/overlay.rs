use crate::{capture, CapturedImage, Result};

pub fn interactive_capture() -> Result<Option<CapturedImage>> {
    #[cfg(windows)]
    {
        let Some(rect) = windows_overlay::select_region()? else {
            return Ok(None);
        };
        return capture::capture_region(rect).map(Some);
    }

    #[cfg(not(windows))]
    {
        Err(crate::WinScreenError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod windows_overlay {
    use crate::{platform, Rect, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, FrameRect,
        GetStockObject, InvalidateRect, SelectObject, SetBkMode, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT,
        PS_SOLID, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, LoadCursorW, PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes,
        SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA,
        IDC_CROSS, LWA_ALPHA, MSG, SW_SHOW, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    const CLASS_NAME: &str = "WinScreenRegionOverlay";

    #[derive(Debug)]
    struct OverlayState {
        virtual_rect: Rect,
        start: Option<POINT>,
        current: POINT,
        result: Option<Rect>,
        canceled: bool,
    }

    pub fn select_region() -> Result<Option<Rect>> {
        platform::set_process_dpi_aware().ok();
        let virtual_rect = platform::virtual_screen_rect()?;
        let class_name = wide(CLASS_NAME);
        let hinstance = unsafe { GetModuleHandleW(None) }?;

        let cursor = unsafe { LoadCursorW(None, IDC_CROSS) }?;
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            hCursor: cursor,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&wnd_class);
        }

        let mut state = OverlayState {
            virtual_rect,
            start: None,
            current: POINT {
                x: virtual_rect.x,
                y: virtual_rect.y,
            },
            result: None,
            canceled: false,
        };

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(wide("win-screen overlay").as_ptr()),
                WS_POPUP | WS_VISIBLE,
                virtual_rect.x,
                virtual_rect.y,
                virtual_rect.width as i32,
                virtual_rect.height as i32,
                None,
                None,
                hinstance,
                None,
            )
        }?;

        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut state as *mut OverlayState) as isize,
            );
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 96, LWA_ALPHA)?;
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if state.canceled {
            Ok(None)
        } else {
            Ok(state.result)
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state_ptr =
            windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
                as *mut OverlayState;

        match msg {
            WM_LBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    let point = cursor_point();
                    state.start = Some(point);
                    state.current = point;
                    SetCapture(hwnd);
                    let _ = InvalidateRect(hwnd, None, true);
                    return LRESULT(0);
                }
            }
            WM_MOUSEMOVE => {
                if let Some(state) = state_ptr.as_mut() {
                    if state.start.is_some() {
                        state.current = cursor_point();
                        let _ = InvalidateRect(hwnd, None, true);
                    }
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP => {
                if let Some(state) = state_ptr.as_mut() {
                    state.current = cursor_point();
                    state.result = selection_rect(state);
                    let _ = ReleaseCapture();
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
            }
            WM_RBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    state.canceled = true;
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
            }
            WM_KEYDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    if wparam.0 == VK_ESCAPE.0 as usize {
                        state.canceled = true;
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                    if wparam.0 == VK_RETURN.0 as usize {
                        state.result = selection_rect(state);
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                }
            }
            WM_PAINT => {
                if let Some(state) = state_ptr.as_mut() {
                    paint(hwnd, state);
                    return LRESULT(0);
                }
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    unsafe fn paint(hwnd: HWND, state: &OverlayState) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);

        let bg = CreateSolidBrush(COLORREF(0x000000));
        let client = RECT {
            left: 0,
            top: 0,
            right: state.virtual_rect.width as i32,
            bottom: state.virtual_rect.height as i32,
        };
        let _ = FillRect(hdc, &client, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        if let Some(selection) = selection_rect(state) {
            let local = RECT {
                left: selection.x - state.virtual_rect.x,
                top: selection.y - state.virtual_rect.y,
                right: selection.x - state.virtual_rect.x + selection.width as i32,
                bottom: selection.y - state.virtual_rect.y + selection.height as i32,
            };

            let white = CreateSolidBrush(COLORREF(0x00FFFFFF));
            let _ = FrameRect(hdc, &local, white);
            let _ = DeleteObject(HGDIOBJ(white.0));

            let pen = CreatePen(PS_SOLID, 2, COLORREF(0x0000D7FF));
            let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            SetBkMode(hdc, TRANSPARENT);
            let _ = windows::Win32::Graphics::Gdi::Rectangle(
                hdc,
                local.left,
                local.top,
                local.right,
                local.bottom,
            );
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }

        let _ = EndPaint(hwnd, &ps);
    }

    fn selection_rect(state: &OverlayState) -> Option<Rect> {
        let start = state.start?;
        let left = start.x.min(state.current.x);
        let top = start.y.min(state.current.y);
        let right = start.x.max(state.current.x);
        let bottom = start.y.max(state.current.y);
        let width = u32::try_from(right - left).ok()?;
        let height = u32::try_from(bottom - top).ok()?;
        if width < 2 || height < 2 {
            return None;
        }
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    }

    unsafe fn cursor_point() -> POINT {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        point
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
