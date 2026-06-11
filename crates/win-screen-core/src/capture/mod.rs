use crate::{CapturedImage, Rect, Result, WinScreenError};

#[cfg(windows)]
mod windows_gdi;

pub fn capture_fullscreen() -> Result<CapturedImage> {
    #[cfg(windows)]
    {
        return windows_gdi::capture_fullscreen();
    }

    #[cfg(not(windows))]
    {
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn capture_region(rect: Rect) -> Result<CapturedImage> {
    if rect.width == 0 || rect.height == 0 {
        return Err(WinScreenError::InvalidRect(rect));
    }

    #[cfg(windows)]
    {
        return windows_gdi::capture_region(rect);
    }

    #[cfg(not(windows))]
    {
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn capture_monitor(id: u32) -> Result<CapturedImage> {
    #[cfg(windows)]
    {
        return windows_gdi::capture_monitor(id);
    }

    #[cfg(not(windows))]
    {
        let _ = id;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn capture_window(hwnd: isize) -> Result<CapturedImage> {
    #[cfg(windows)]
    {
        return windows_gdi::capture_window(hwnd);
    }

    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err(WinScreenError::UnsupportedPlatform)
    }
}
