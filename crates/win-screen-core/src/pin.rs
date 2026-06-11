use crate::{CapturedImage, PinHandle, PinInfo, Result};

#[cfg(windows)]
mod windows_pin;

pub fn pin_image(image: CapturedImage) -> Result<PinHandle> {
    #[cfg(windows)]
    {
        return windows_pin::pin_image(image);
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        let _ = image;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn close_pin(id: u64) -> Result<()> {
    #[cfg(windows)]
    {
        return windows_pin::close_pin(id);
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        let _ = id;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn set_pin_opacity(id: u64, opacity: f32) -> Result<()> {
    #[cfg(windows)]
    {
        return windows_pin::set_pin_opacity(id, opacity);
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        let _ = (id, opacity);
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn list_pins() -> Result<Vec<PinInfo>> {
    #[cfg(windows)]
    {
        return windows_pin::list_pins();
    }

    #[cfg(not(windows))]
    {
        use crate::WinScreenError;
        Err(WinScreenError::UnsupportedPlatform)
    }
}
