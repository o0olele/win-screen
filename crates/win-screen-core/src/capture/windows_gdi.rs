use crate::{CapturedImage, MonitorInfo, Rect, Result, WinScreenError, WindowInfo};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    EnumDisplayMonitors, GetDIBits, GetMonitorInfoW, GetWindowDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, HMONITOR,
    MONITORINFO, SRCCOPY,
};

#[link(name = "user32")]
extern "system" {
    fn PrintWindow(hwnd: HWND, hdc: HDC, nflags: u32) -> BOOL;
}
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetDesktopWindow, GetSystemMetrics, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, IsIconic, IsWindowVisible, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

struct ScreenDc {
    hwnd: HWND,
    hdc: HDC,
}

impl ScreenDc {
    fn new(hwnd: HWND) -> Result<Self> {
        let hdc = unsafe { GetWindowDC(hwnd) };
        if hdc.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "GetWindowDC failure mapping",
            });
        }
        Ok(Self { hwnd, hdc })
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::Graphics::Gdi::ReleaseDC(self.hwnd, self.hdc);
        }
    }
}

struct MemDc(HDC);

impl MemDc {
    fn new(src: HDC) -> Result<Self> {
        let dc = unsafe { CreateCompatibleDC(src) };
        if dc.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "CreateCompatibleDC failure mapping",
            });
        }
        Ok(Self(dc))
    }
}

impl Drop for MemDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct Bitmap(HBITMAP);

impl Bitmap {
    fn new(dc: HDC, width: i32, height: i32) -> Result<Self> {
        let bitmap = unsafe { CreateCompatibleBitmap(dc, width, height) };
        if bitmap.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "CreateCompatibleBitmap failure mapping",
            });
        }
        Ok(Self(bitmap))
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0 .0));
        }
    }
}

struct SelectedObject {
    dc: HDC,
    old: HGDIOBJ,
}

impl SelectedObject {
    fn new(dc: HDC, bitmap: HBITMAP) -> Result<Self> {
        let old = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        if old.0.is_null() {
            return Err(WinScreenError::NotImplemented {
                feature: "SelectObject failure mapping",
            });
        }
        Ok(Self { dc, old })
    }
}

impl Drop for SelectedObject {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
        }
    }
}

/// Extract a rectangular region from an existing in-memory DC as a `CapturedImage`.
/// `offset_x`/`offset_y` are the top-left corner in the DC's local coordinate space
/// (i.e. screen-absolute minus the DC's own origin). Used by the overlay to read from
/// the frozen screenshot captured at overlay-start time instead of re-capturing the
/// live screen at confirmation time.
pub(crate) fn extract_region_from_dc(
    src_dc: HDC,
    offset_x: i32,
    offset_y: i32,
    width: u32,
    height: u32,
) -> Result<CapturedImage> {
    if width == 0 || height == 0 {
        return Err(WinScreenError::InvalidRect(crate::Rect {
            x: offset_x,
            y: offset_y,
            width,
            height,
        }));
    }
    let w = i32::try_from(width).map_err(|_| WinScreenError::ImageTooLarge)?;
    let h = i32::try_from(height).map_err(|_| WinScreenError::ImageTooLarge)?;

    let desktop = unsafe { GetDesktopWindow() };
    let screen_dc = ScreenDc::new(desktop)?;
    let mem_dc = MemDc::new(screen_dc.hdc)?;
    let bitmap = Bitmap::new(screen_dc.hdc, w, h)?;
    let _selected = SelectedObject::new(mem_dc.0, bitmap.0)?;

    // src_dc is an in-memory DC (not the live screen), so CAPTUREBLT is not needed.
    let copied = unsafe { BitBlt(mem_dc.0, 0, 0, w, h, src_dc, offset_x, offset_y, SRCCOPY) };
    if copied.is_err() {
        return Err(WinScreenError::NotImplemented {
            feature: "BitBlt extract from frozen DC",
        });
    }

    bitmap_to_rgba(screen_dc.hdc, bitmap.0, width, height)
}

pub fn capture_fullscreen() -> Result<CapturedImage> {
    let rect = virtual_screen_rect();
    capture_region(rect)
}

pub fn capture_region(rect: Rect) -> Result<CapturedImage> {
    let width = i32::try_from(rect.width).map_err(|_| WinScreenError::InvalidRect(rect))?;
    let height = i32::try_from(rect.height).map_err(|_| WinScreenError::InvalidRect(rect))?;

    let desktop = unsafe { GetDesktopWindow() };
    let screen_dc = ScreenDc::new(desktop)?;
    let mem_dc = MemDc::new(screen_dc.hdc)?;
    let bitmap = Bitmap::new(screen_dc.hdc, width, height)?;
    let _selected = SelectedObject::new(mem_dc.0, bitmap.0)?;

    let copied = unsafe {
        BitBlt(
            mem_dc.0,
            0,
            0,
            width,
            height,
            screen_dc.hdc,
            rect.x,
            rect.y,
            SRCCOPY | CAPTUREBLT,
        )
    };
    if copied.is_err() {
        return Err(WinScreenError::NotImplemented {
            feature: "BitBlt failure mapping",
        });
    }

    bitmap_to_rgba(screen_dc.hdc, bitmap.0, rect.width, rect.height)
}

pub fn capture_monitor(id: u32) -> Result<CapturedImage> {
    let monitors = enumerate_monitors()?;
    let rect = monitors
        .get(id as usize)
        .copied()
        .ok_or(WinScreenError::NotImplemented {
            feature: "monitor id outside available display list",
        })?;
    capture_region(rect)
}

pub fn capture_window(hwnd: isize) -> Result<CapturedImage> {
    let hwnd = HWND(hwnd as *mut c_void);
    let rect = window_bounds(hwnd)?;
    let width = u32::try_from(rect.right - rect.left).map_err(|_| {
        WinScreenError::InvalidRect(Rect {
            x: rect.left,
            y: rect.top,
            width: 0,
            height: 0,
        })
    })?;
    let height = u32::try_from(rect.bottom - rect.top).map_err(|_| {
        WinScreenError::InvalidRect(Rect {
            x: rect.left,
            y: rect.top,
            width: 0,
            height: 0,
        })
    })?;

    if let Ok(image) = print_window(hwnd, width, height) {
        return Ok(image);
    }

    capture_region(Rect::new(rect.left, rect.top, width, height)?)
}

pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
    Ok(enumerate_monitor_infos()?)
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, data: LPARAM) -> BOOL {
        let windows = &mut *(data.0 as *mut Vec<WindowInfo>);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        if IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }

        let title_len = GetWindowTextLengthW(hwnd);
        if title_len <= 0 {
            return BOOL(1);
        }

        let mut title = vec![0_u16; title_len as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut title);
        if copied <= 0 {
            return BOOL(1);
        }
        title.truncate(copied as usize);
        let title = String::from_utf16_lossy(&title).trim().to_string();
        if title.is_empty() {
            return BOOL(1);
        }

        let Ok(rect) = window_bounds(hwnd) else {
            return BOOL(1);
        };

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return BOOL(1);
        }
        if !rect_intersects_virtual_screen(rect) {
            return BOOL(1);
        }

        windows.push(WindowInfo {
            hwnd: hwnd.0 as isize,
            title,
            rect: Rect {
                x: rect.left,
                y: rect.top,
                width: width as u32,
                height: height as u32,
            },
        });

        BOOL(1)
    }

    let mut windows = Vec::new();
    unsafe {
        EnumWindows(
            Some(enum_proc),
            LPARAM((&mut windows as *mut Vec<WindowInfo>) as isize),
        )?;
    }
    Ok(windows)
}

fn print_window(hwnd: HWND, width: u32, height: u32) -> Result<CapturedImage> {
    let width_i32 = i32::try_from(width).map_err(|_| WinScreenError::ImageTooLarge)?;
    let height_i32 = i32::try_from(height).map_err(|_| WinScreenError::ImageTooLarge)?;

    let window_dc = ScreenDc::new(hwnd)?;
    let mem_dc = MemDc::new(window_dc.hdc)?;
    let bitmap = Bitmap::new(window_dc.hdc, width_i32, height_i32)?;
    let _selected = SelectedObject::new(mem_dc.0, bitmap.0)?;

    let printed = unsafe { PrintWindow(hwnd, mem_dc.0, 0) };
    if !printed.as_bool() {
        return Err(WinScreenError::NotImplemented {
            feature: "PrintWindow returned false",
        });
    }

    bitmap_to_rgba(window_dc.hdc, bitmap.0, width, height)
}

fn window_bounds(hwnd: HWND) -> Result<RECT> {
    let mut rect = RECT::default();
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
    };
    if dwm.is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        return Ok(rect);
    }

    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|_| WinScreenError::NotImplemented {
        feature: "GetWindowRect failure mapping",
    })?;
    Ok(rect)
}

fn rect_intersects_virtual_screen(rect: RECT) -> bool {
    let screen = virtual_screen_rect();
    let screen_right = screen.x + screen.width as i32;
    let screen_bottom = screen.y + screen.height as i32;
    rect.left < screen_right
        && rect.right > screen.x
        && rect.top < screen_bottom
        && rect.bottom > screen.y
}

fn virtual_screen_rect() -> Rect {
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } as u32;
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) } as u32;
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn bitmap_to_rgba(dc: HDC, bitmap: HBITMAP, width: u32, height: u32) -> Result<CapturedImage> {
    let len = width
        .checked_mul(height)
        .and_then(|px| px.checked_mul(4))
        .ok_or(WinScreenError::ImageTooLarge)? as usize;
    let mut bgra = vec![0_u8; len];

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let rows = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(bgra.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if rows == 0 {
        return Err(WinScreenError::NotImplemented {
            feature: "GetDIBits failure mapping",
        });
    }

    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }

    CapturedImage::new(width, height, bgra)
}

fn enumerate_monitors() -> Result<Vec<Rect>> {
    Ok(enumerate_monitor_infos()?
        .into_iter()
        .map(|monitor| monitor.rect)
        .collect())
}

fn enumerate_monitor_infos() -> Result<Vec<MonitorInfo>> {
    unsafe extern "system" fn enum_proc(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<MonitorInfo>);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let id = monitors.len() as u32;
            let width = (info.rcMonitor.right - info.rcMonitor.left).max(0) as u32;
            let height = (info.rcMonitor.bottom - info.rcMonitor.top).max(0) as u32;
            monitors.push(MonitorInfo {
                id,
                primary: (info.dwFlags & 1) != 0,
                rect: Rect {
                    x: info.rcMonitor.left,
                    y: info.rcMonitor.top,
                    width,
                    height,
                },
            });
        }
        BOOL(1)
    }

    let mut monitors = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            HDC(null_mut()),
            None,
            Some(enum_proc),
            LPARAM((&mut monitors as *mut Vec<MonitorInfo>) as isize),
        )
    };
    if !ok.as_bool() || monitors.is_empty() {
        monitors.push(MonitorInfo {
            id: 0,
            primary: true,
            rect: virtual_screen_rect(),
        });
    }
    Ok(monitors)
}
