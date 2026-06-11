use crate::{CapturedImage, Rect, Result, WinScreenError};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    EnumDisplayMonitors, GetDIBits, GetMonitorInfoW, GetWindowDC, MonitorFromWindow, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetSystemMetrics, GetWindowRect, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
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
    let mut rect = RECT::default();
    let ok = unsafe { GetWindowRect(hwnd, &mut rect) };
    if ok.is_err() {
        return Err(WinScreenError::NotImplemented {
            feature: "GetWindowRect failure mapping",
        });
    }

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
    capture_region(Rect::new(rect.left, rect.top, width, height)?)
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
    unsafe extern "system" fn enum_proc(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<Rect>);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let width = (info.rcMonitor.right - info.rcMonitor.left).max(0) as u32;
            let height = (info.rcMonitor.bottom - info.rcMonitor.top).max(0) as u32;
            monitors.push(Rect {
                x: info.rcMonitor.left,
                y: info.rcMonitor.top,
                width,
                height,
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
            LPARAM((&mut monitors as *mut Vec<Rect>) as isize),
        )
    };
    if !ok.as_bool() {
        let desktop = unsafe { GetDesktopWindow() };
        let monitor = unsafe { MonitorFromWindow(desktop, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            monitors.push(Rect {
                x: info.rcMonitor.left,
                y: info.rcMonitor.top,
                width: (info.rcMonitor.right - info.rcMonitor.left) as u32,
                height: (info.rcMonitor.bottom - info.rcMonitor.top) as u32,
            });
        }
    }
    Ok(monitors)
}
