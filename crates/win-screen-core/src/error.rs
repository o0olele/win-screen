use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum WinScreenError {
    #[error("win-screen currently requires Windows for this operation")]
    UnsupportedPlatform,

    #[error("{feature} is not implemented yet")]
    NotImplemented { feature: &'static str },

    #[error("invalid image buffer: {width}x{height} needs {expected} RGBA bytes, got {actual}")]
    InvalidImageBuffer {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },

    #[error("invalid rectangle: {0:?}")]
    InvalidRect(crate::api::Rect),

    #[error("image dimensions overflow supported buffer size")]
    ImageTooLarge,

    #[error("clipboard error: {0}")]
    Clipboard(String),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[cfg(windows)]
    #[error("windows api error: {0}")]
    Windows(#[from] windows::core::Error),
}

pub type Result<T> = std::result::Result<T, WinScreenError>;
