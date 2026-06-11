use crate::{Rect, Result};

pub fn set_process_dpi_aware() -> Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };

        let ok =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if ok.is_err() {
            tracing::debug!("SetProcessDpiAwarenessContext did not apply; continuing");
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn virtual_screen_rect() -> Result<Rect> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };

        return Ok(Rect {
            x: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
            y: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
            width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } as u32,
            height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) } as u32,
        });
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        Err(WinScreenError::UnsupportedPlatform)
    }
}
