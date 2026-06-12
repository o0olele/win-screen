use crate::{capture, CapturedImage, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionDecision {
    Confirm,
    Pin,
    Cancel,
}

pub fn interactive_capture() -> Result<Option<CapturedImage>> {
    Ok(interactive_capture_selection()?.map(|(_, image)| image))
}

pub fn interactive_capture_selection() -> Result<Option<(crate::Rect, CapturedImage)>> {
    Ok(interactive_capture_selection_with_decision(|_| {
        SelectionDecision::Confirm
    })?
    .map(|(rect, image, _)| (rect, image)))
}

pub fn interactive_capture_selection_with_decision<F>(
    decide: F,
) -> Result<Option<(crate::Rect, CapturedImage, SelectionDecision)>>
where
    F: FnOnce(crate::Rect) -> SelectionDecision + 'static,
{
    #[cfg(windows)]
    {
        let Some((rect, decision)) = windows_overlay::select_region(decide)? else {
            return Ok(None);
        };
        if matches!(decision, SelectionDecision::Cancel) {
            return Ok(None);
        }
        let image = capture::capture_region(rect)?;
        return Ok(Some((rect, image, decision)));
    }

    #[cfg(not(windows))]
    {
        Err(crate::WinScreenError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
mod windows_overlay {
    use super::SelectionDecision;
    use crate::{platform, Rect, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen,
        CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect,
        GetStockObject, InvalidateRect, SelectObject, SetBkColor, SetBkMode, SetTextColor,
        DT_CENTER, DT_SINGLELINE, DT_VCENTER, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY,
        TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
        GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible,
        LoadCursorW, PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes,
        SetWindowLongPtrW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA,
        IDC_CROSS, LWA_ALPHA, LWA_COLORKEY, MSG, SW_SHOW, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW,
        WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    const CLASS_NAME: &str = "WinScreenRegionOverlay";
    const COLOR_KEY: COLORREF = COLORREF(0x00010203);
    struct OverlayState {
        virtual_rect: Rect,
        start: Option<POINT>,
        current: POINT,
        hover: Option<Rect>,
        result: Option<Rect>,
        decision: SelectionDecision,
        decide: Option<Box<dyn FnOnce(Rect) -> SelectionDecision>>,
        canceled: bool,
    }

    pub fn select_region<F>(decide: F) -> Result<Option<(Rect, SelectionDecision)>>
    where
        F: FnOnce(Rect) -> SelectionDecision + 'static,
    {
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
            hover: None,
            result: None,
            decision: SelectionDecision::Confirm,
            decide: Some(Box::new(decide)),
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
            SetLayeredWindowAttributes(hwnd, COLOR_KEY, 118, LWA_ALPHA | LWA_COLORKEY)?;
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
            Ok(state.result.map(|rect| (rect, state.decision)))
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState;

        match msg {
            WM_LBUTTONDOWN => {
                if let Some(state) = state_ptr.as_mut() {
                    if state.result.is_some() {
                        return LRESULT(0);
                    }
                    let point = cursor_point();
                    state.start = Some(point);
                    state.current = point;
                    SetCapture(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }
            }
            WM_MOUSEMOVE => {
                if let Some(state) = state_ptr.as_mut() {
                    if state.result.is_some() {
                        return LRESULT(0);
                    }
                    if state.start.is_some() {
                        state.current = cursor_point();
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        let next_hover = window_rect_from_point(hwnd, cursor_point());
                        if state.hover != next_hover {
                            state.hover = next_hover;
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                    }
                    return LRESULT(0);
                }
            }
            WM_LBUTTONUP => {
                if let Some(state) = state_ptr.as_mut() {
                    state.current = cursor_point();
                    if let Some(rect) = selection_rect(state).or(state.hover) {
                        state.result = Some(rect);
                        state.start = None;
                        state.hover = None;
                        let _ = ReleaseCapture();
                        let _ = InvalidateRect(hwnd, None, false);
                        let decision = state
                            .decide
                            .take()
                            .map(|decide| decide(rect))
                            .unwrap_or(SelectionDecision::Confirm);
                        state.decision = decision;
                        if matches!(decision, SelectionDecision::Cancel) {
                            state.canceled = true;
                        }
                        let _ = DestroyWindow(hwnd);
                    }
                    let _ = ReleaseCapture();
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
                        if let Some(rect) = selection_rect(state).or(state.result) {
                            state.result = Some(rect);
                            state.decision = state
                                .decide
                                .take()
                                .map(|decide| decide(rect))
                                .unwrap_or(SelectionDecision::Confirm);
                        }
                        let _ = DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                }
            }
            WM_PAINT => {
                if let Some(state) = state_ptr.as_ref() {
                    paint(hwnd, state);
                    return LRESULT(0);
                }
            }
            WM_ERASEBKGND => {
                return LRESULT(1);
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
        let mem_dc = CreateCompatibleDC(hdc);
        if mem_dc.0.is_null() {
            let _ = EndPaint(hwnd, &ps);
            return;
        }

        let bitmap = CreateCompatibleBitmap(
            hdc,
            state.virtual_rect.width as i32,
            state.virtual_rect.height as i32,
        );
        if bitmap.0.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = EndPaint(hwnd, &ps);
            return;
        }

        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

        let bg = CreateSolidBrush(COLORREF(0x000000));
        let client = RECT {
            left: 0,
            top: 0,
            right: state.virtual_rect.width as i32,
            bottom: state.virtual_rect.height as i32,
        };
        let _ = FillRect(mem_dc, &client, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        let active = state.result.or_else(|| selection_rect(state)).or(state.hover);
        if let Some(selection) = active {
            let local = RECT {
                left: selection.x - state.virtual_rect.x,
                top: selection.y - state.virtual_rect.y,
                right: selection.x - state.virtual_rect.x + selection.width as i32,
                bottom: selection.y - state.virtual_rect.y + selection.height as i32,
            };

            let hole = CreateSolidBrush(COLOR_KEY);
            let _ = FillRect(mem_dc, &local, hole);
            let _ = DeleteObject(HGDIOBJ(hole.0));

            let white = CreateSolidBrush(COLORREF(0x00FFFFFF));
            let _ = FrameRect(mem_dc, &local, white);
            let _ = DeleteObject(HGDIOBJ(white.0));

            let is_drag = state.result.is_none() && selection_rect(state).is_some();
            let pen_color = if is_drag {
                COLORREF(0x0000D7FF)
            } else {
                COLORREF(0x0000FF66)
            };
            let pen = CreatePen(PS_SOLID, 2, pen_color);
            let old_pen = SelectObject(mem_dc, HGDIOBJ(pen.0));
            let old_brush = SelectObject(mem_dc, GetStockObject(NULL_BRUSH));
            SetBkMode(mem_dc, TRANSPARENT);
            let _ = windows::Win32::Graphics::Gdi::Rectangle(
                mem_dc,
                local.left,
                local.top,
                local.right,
                local.bottom,
            );
            SelectObject(mem_dc, old_brush);
            SelectObject(mem_dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(pen.0));

            if is_drag {
                draw_size_label(mem_dc, &local, selection.width, selection.height);
            }
        }

        let _ = BitBlt(
            hdc,
            0,
            0,
            state.virtual_rect.width as i32,
            state.virtual_rect.height as i32,
            mem_dc,
            0,
            0,
            SRCCOPY,
        );
        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }

    unsafe fn draw_size_label(
        hdc: windows::Win32::Graphics::Gdi::HDC,
        rect: &RECT,
        width: u32,
        height: u32,
    ) {
        let label = format!("{width} x {height}");
        let mut text = wide(&label);
        let label_width = (label.len() as i32 * 8).max(76);
        let mut label_rect = RECT {
            left: rect.left,
            top: rect.top - 27,
            right: rect.left + label_width,
            bottom: rect.top - 5,
        };
        if label_rect.top < 4 {
            label_rect.top = rect.bottom + 5;
            label_rect.bottom = rect.bottom + 27;
        }

        let bg = CreateSolidBrush(COLORREF(0x00202020));
        let _ = FillRect(hdc, &label_rect, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        SetBkMode(hdc, TRANSPARENT);
        SetBkColor(hdc, COLORREF(0x00202020));
        SetTextColor(hdc, COLORREF(0x00FFFFFF));
        let _ = DrawTextW(
            hdc,
            &mut text,
            &mut label_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
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

    unsafe fn window_rect_from_point(overlay_hwnd: HWND, point: POINT) -> Option<Rect> {
        struct HitTest {
            overlay_hwnd: HWND,
            point: POINT,
            result: Option<Rect>,
        }

        unsafe extern "system" fn enum_proc(
            hwnd: HWND,
            data: LPARAM,
        ) -> windows::Win32::Foundation::BOOL {
            let hit = &mut *(data.0 as *mut HitTest);
            if hwnd == hit.overlay_hwnd
                || !IsWindowVisible(hwnd).as_bool()
                || IsIconic(hwnd).as_bool()
            {
                return windows::Win32::Foundation::BOOL(1);
            }

            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return windows::Win32::Foundation::BOOL(1);
            }

            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width < 32 || height < 32 {
                return windows::Win32::Foundation::BOOL(1);
            }

            if hit.point.x >= rect.left
                && hit.point.x <= rect.right
                && hit.point.y >= rect.top
                && hit.point.y <= rect.bottom
            {
                hit.result = Some(Rect {
                    x: rect.left,
                    y: rect.top,
                    width: width as u32,
                    height: height as u32,
                });
                return windows::Win32::Foundation::BOOL(0);
            }

            windows::Win32::Foundation::BOOL(1)
        }

        let mut hit = HitTest {
            overlay_hwnd,
            point,
            result: None,
        };
        let _ = EnumWindows(Some(enum_proc), LPARAM((&mut hit as *mut HitTest) as isize));
        hit.result
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}
