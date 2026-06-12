//! Core library for Windows screenshot, recording, and desktop pinning.

pub mod annotate;
pub mod api;
pub mod capture;
pub mod error;
pub mod io;
pub mod overlay;
pub mod pin;
pub mod platform;
pub mod record;

pub use api::{
    AudioOptions, CapturedImage, Capturer, InteractiveCaptureOptions, MonitorInfo, Pin, PinHandle,
    PinInfo, Recorder, RecordingHandle, RecordingOptions, RecordingTarget, Rect, Screenshot, Size,
    WinScreenEvent, WindowInfo,
};
pub use error::{Result, WinScreenError};
