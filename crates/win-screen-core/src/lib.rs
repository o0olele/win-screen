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

pub use overlay::RegionIndicator;
#[cfg(windows)]
pub use overlay::post_overlay_command;
pub use annotate::{
    Annotation, AnnotationCommand, AnnotationDocument, AnnotationEditAction, AnnotationEditResult,
    AnnotationShape, AnnotationTool, Color, Point,
};
#[cfg(windows)]
pub use annotate::{edit_image_with_overlay, post_annotation_command, AnnotationOverlay};
pub use api::{
    AudioOptions, CapturedImage, Capturer, InteractiveCaptureOptions, MonitorInfo, Pin, PinHandle,
    PinInfo, Recorder, RecordingHandle, RecordingOptions, RecordingTarget, Rect, Screenshot, Size,
    WinScreenEvent, WindowInfo,
};
pub use error::{Result, WinScreenError};
pub use platform::{hotkey_mod, register_hotkey, HotkeyHandle};
