use crate::{platform, CapturedImage, PinHandle, PinInfo, Result, Size, WinScreenError};
use std::collections::HashMap;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint, FillRect,
    GetStockObject, InvalidateRect, SelectObject, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HBRUSH, HDC, HGDIOBJ, PAINTSTRUCT, SRCCOPY, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HTCAPTION, IDC_ARROW, LWA_ALPHA, MSG,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW, WM_APP, WM_DESTROY, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT, WM_RBUTTONDOWN, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const CLASS_NAME: &str = "WinScreenPinWindow";
const WM_PIN_SET_OPACITY: u32 = WM_APP + 1;

#[derive(Clone, Copy)]
struct PinEntry {
    hwnd: HWND,
    size: Size,
}

unsafe impl Send for PinEntry {}

struct PinWindowState {
    id: u64,
    image: CapturedImage,
    bitmap: HBITMAP,
    bitmap_dc: HDC,
    original_width: i32,
    original_height: i32,
    opacity: u8,
}

pub fn pin_image(image: CapturedImage) -> Result<PinHandle> {
    let handle = PinHandle::next();
    let id = handle.id();
    let (tx, rx) = crossbeam_channel::bounded(1);

    thread::Builder::new()
        .name(format!("win-screen-pin-{id}"))
        .spawn(move || {
            let startup = tx.clone();
            if let Err(err) = run_pin_window(id, image, startup) {
                let _ = tx.send(Err(err));
            }
        })
        .map_err(|_| WinScreenError::NotImplemented {
            feature: "spawn pin window thread",
        })?;

    let entry =
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| WinScreenError::NotImplemented {
                feature: "pin window startup timed out",
            })??;

    registry()
        .lock()
        .expect("pin registry poisoned")
        .insert(id, entry);
    Ok(handle)
}

pub fn close_pin(id: u64) -> Result<()> {
    let Some(entry) = registry()
        .lock()
        .expect("pin registry poisoned")
        .remove(&id)
    else {
        return Ok(());
    };

    unsafe {
        PostMessageW(entry.hwnd, WM_RBUTTONDOWN, WPARAM(0), LPARAM(0))?;
    }
    Ok(())
}

pub fn set_pin_opacity(id: u64, opacity: f32) -> Result<()> {
    let entry = registry()
        .lock()
        .expect("pin registry poisoned")
        .get(&id)
        .copied()
        .ok_or(WinScreenError::NotImplemented {
            feature: "pin handle not found",
        })?;

    let alpha = (opacity.clamp(0.1, 1.0) * 255.0).round() as usize;
    unsafe {
        PostMessageW(entry.hwnd, WM_PIN_SET_OPACITY, WPARAM(alpha), LPARAM(0))?;
    }
    Ok(())
}

pub fn list_pins() -> Result<Vec<PinInfo>> {
    Ok(registry()
        .lock()
        .expect("pin registry poisoned")
        .iter()
        .map(|(id, entry)| PinInfo {
            id: *id,
            size: entry.size,
        })
        .collect())
}

fn registry() -> &'static Mutex<HashMap<u64, PinEntry>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, PinEntry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn run_pin_window(
    id: u64,
    image: CapturedImage,
    startup: crossbeam_channel::Sender<Result<PinEntry>>,
) -> Result<()> {
    platform::set_process_dpi_aware().ok();

    let class_name = wide(CLASS_NAME);
    let hinstance = unsafe { GetModuleHandleW(None) }?;
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }?;
    let wnd_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        hCursor: cursor,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&wnd_class);
    }

    let max_w = 720_u32;
    let max_h = 520_u32;
    let scale = (max_w as f32 / image.width as f32)
        .min(max_h as f32 / image.height as f32)
        .min(1.0);
    let window_w = (image.width as f32 * scale).round().max(80.0) as i32;
    let window_h = (image.height as f32 * scale).round().max(60.0) as i32;
    let virtual_rect = platform::virtual_screen_rect()?;

    let (bitmap, bitmap_dc) = create_bitmap(&image)?;
    let mut state = Box::new(PinWindowState {
        id,
        image,
        bitmap,
        bitmap_dc,
        original_width: window_w,
        original_height: window_h,
        opacity: 255,
    });

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(wide("win-screen pin").as_ptr()),
            WS_POPUP | WS_VISIBLE,
            virtual_rect.x + 80,
            virtual_rect.y + 80,
            window_w,
            window_h,
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
            (&mut *state as *mut PinWindowState) as isize,
        );
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = InvalidateRect(hwnd, None, true);
    }

    let _ = startup.send(Ok(PinEntry {
        hwnd,
        size: state.image.size(),
    }));

    let leaked_state = Box::into_raw(state);
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        drop(Box::from_raw(leaked_state));
    }

    Ok(())
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PinWindowState;

    match msg {
        WM_PAINT => {
            if let Some(state) = state_ptr.as_ref() {
                paint(hwnd, state);
                return LRESULT(0);
            }
        }
        WM_LBUTTONDOWN => {
            ReleaseCapture().ok();
            PostMessageW(
                hwnd,
                WM_NCLBUTTONDOWN,
                WPARAM(HTCAPTION as usize),
                LPARAM(0),
            )
            .ok();
            return LRESULT(0);
        }
        WM_LBUTTONDBLCLK => {
            if let Some(state) = state_ptr.as_ref() {
                resize_window(hwnd, state.original_width, state.original_height);
                return LRESULT(0);
            }
        }
        WM_MOUSEWHEEL => {
            if let Some(state) = state_ptr.as_mut() {
                let delta = wheel_delta(wparam);
                if ctrl_pressed(wparam) {
                    let next = (state.opacity as i16 + if delta > 0 { 18 } else { -18 })
                        .clamp(38, 255) as u8;
                    state.opacity = next;
                    SetLayeredWindowAttributes(hwnd, COLORREF(0), next, LWA_ALPHA).ok();
                } else {
                    zoom_window(hwnd, delta);
                }
                return LRESULT(0);
            }
        }
        WM_RBUTTONDOWN => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_PIN_SET_OPACITY => {
            let alpha = wparam.0.clamp(26, 255) as u8;
            if let Some(state) = state_ptr.as_mut() {
                state.opacity = alpha;
            }
            SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA).ok();
            return LRESULT(0);
        }
        WM_DESTROY => {
            if let Some(state) = state_ptr.as_ref() {
                registry()
                    .lock()
                    .expect("pin registry poisoned")
                    .remove(&state.id);
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn paint(hwnd: HWND, state: &PinWindowState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let _ = FillRect(hdc, &client, HBRUSH(GetStockObject(WHITE_BRUSH).0));
    let _ = StretchBlt(
        hdc,
        0,
        0,
        client.right - client.left,
        client.bottom - client.top,
        state.bitmap_dc,
        0,
        0,
        state.image.width as i32,
        state.image.height as i32,
        SRCCOPY,
    );

    let _ = EndPaint(hwnd, &ps);
}

unsafe fn zoom_window(hwnd: HWND, delta: i16) {
    let mut client = RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() {
        return;
    }

    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width <= 0 || height <= 0 {
        return;
    }

    let factor = if delta > 0 { 1.1_f32 } else { 0.9_f32 };
    let next_w = ((width as f32 * factor).round() as i32).clamp(80, 2400);
    let next_h = ((height as f32 * factor).round() as i32).clamp(60, 1800);
    resize_window(hwnd, next_w, next_h);
}

unsafe fn resize_window(hwnd: HWND, width: i32, height: i32) {
    SetWindowPos(
        hwnd,
        None,
        0,
        0,
        width,
        height,
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
    )
    .ok();
}

fn wheel_delta(wparam: WPARAM) -> i16 {
    ((wparam.0 >> 16) & 0xffff) as u16 as i16
}

fn ctrl_pressed(wparam: WPARAM) -> bool {
    const MK_CONTROL: usize = 0x0008;
    (wparam.0 & MK_CONTROL) != 0
}

fn create_bitmap(image: &CapturedImage) -> Result<(HBITMAP, HDC)> {
    let bitmap_dc = unsafe { CreateCompatibleDC(HDC(null_mut())) };
    if bitmap_dc.0.is_null() {
        return Err(WinScreenError::NotImplemented {
            feature: "CreateCompatibleDC for pin bitmap",
        });
    }

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: image.width as i32,
            biHeight: -(image.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = null_mut();
    let bitmap = unsafe { CreateDIBSection(bitmap_dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
    if bitmap.0.is_null() || bits.is_null() {
        unsafe {
            let _ = DeleteDC(bitmap_dc);
        }
        return Err(WinScreenError::NotImplemented {
            feature: "CreateDIBSection for pin bitmap",
        });
    }

    let mut bgra = image.rgba.clone();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }

    unsafe {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        SelectObject(bitmap_dc, HGDIOBJ(bitmap.0));
    }

    Ok((bitmap, bitmap_dc))
}

impl Drop for PinWindowState {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.bitmap_dc);
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
