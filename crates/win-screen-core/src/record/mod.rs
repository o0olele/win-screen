use crate::{RecordingHandle, RecordingOptions, Result};
#[cfg(not(windows))]
use crate::WinScreenError;
use std::path::PathBuf;

#[cfg(windows)]
mod windows_record;
#[cfg(windows)]
mod wasapi_audio;

pub fn start_recording(options: RecordingOptions) -> Result<RecordingHandle> {
    #[cfg(windows)]
    {
        let handle = RecordingHandle::next();
        let entry = windows_record::start(handle.id(), options)?;
        windows_record::register(handle.id(), entry);
        return Ok(handle);
    }

    #[cfg(not(windows))]
    {
        let _ = options;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn pause_recording(id: u64) -> Result<()> {
    #[cfg(windows)]
    {
        return windows_record::pause(id);
    }

    #[cfg(not(windows))]
    {
        let _ = id;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn resume_recording(id: u64) -> Result<()> {
    #[cfg(windows)]
    {
        return windows_record::resume(id);
    }

    #[cfg(not(windows))]
    {
        let _ = id;
        Err(WinScreenError::UnsupportedPlatform)
    }
}

pub fn stop_recording(id: u64) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        return windows_record::stop(id);
    }

    #[cfg(not(windows))]
    {
        let _ = id;
        Err(WinScreenError::UnsupportedPlatform)
    }
}
