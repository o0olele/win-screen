use crate::{RecordingHandle, RecordingOptions, Result, WinScreenError};
use std::path::PathBuf;

pub fn start_recording(_options: RecordingOptions) -> Result<RecordingHandle> {
    Err(WinScreenError::NotImplemented {
        feature: "WGC/MP4 recording",
    })
}

pub fn pause_recording(_id: u64) -> Result<()> {
    Err(WinScreenError::NotImplemented {
        feature: "recording pause",
    })
}

pub fn resume_recording(_id: u64) -> Result<()> {
    Err(WinScreenError::NotImplemented {
        feature: "recording resume",
    })
}

pub fn stop_recording(_id: u64) -> Result<PathBuf> {
    Err(WinScreenError::NotImplemented {
        feature: "recording stop",
    })
}
