use crate::{capture, io, pin, record, Result, WinScreenError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self> {
        let rect = Self {
            x,
            y,
            width,
            height,
        };
        if width == 0 || height == 0 {
            return Err(WinScreenError::InvalidRect(rect));
        }
        Ok(rect)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CapturedImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let pixels = width
            .checked_mul(height)
            .and_then(|px| px.checked_mul(4))
            .ok_or(WinScreenError::ImageTooLarge)? as usize;

        if rgba.len() != pixels {
            return Err(WinScreenError::InvalidImageBuffer {
                width,
                height,
                expected: pixels,
                actual: rgba.len(),
            });
        }

        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn crop(&self, rect: Rect) -> Result<Self> {
        if rect.width == 0
            || rect.height == 0
            || rect.x < 0
            || rect.y < 0
            || rect.x as u32 >= self.width
            || rect.y as u32 >= self.height
            || rect.x as u32 + rect.width > self.width
            || rect.y as u32 + rect.height > self.height
        {
            return Err(WinScreenError::InvalidRect(rect));
        }

        let mut out = Vec::with_capacity((rect.width * rect.height * 4) as usize);
        let src_stride = self.width as usize * 4;
        let row_bytes = rect.width as usize * 4;
        let start_x = rect.x as usize * 4;
        for y in rect.y as usize..(rect.y as usize + rect.height as usize) {
            let start = y * src_stride + start_x;
            out.extend_from_slice(&self.rgba[start..start + row_bytes]);
        }

        Self::new(rect.width, rect.height, out)
    }

    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<()> {
        io::save_png(self, path)
    }

    pub fn save_jpeg(&self, path: impl AsRef<Path>, quality: u8) -> Result<()> {
        io::save_jpeg(self, path, quality)
    }

    pub fn copy_to_clipboard(&self) -> Result<()> {
        io::write_clipboard_image(self)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        io::load_image(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCaptureOptions {
    pub annotate: bool,
    pub copy_to_clipboard: bool,
    pub save_path: Option<PathBuf>,
}

impl Default for InteractiveCaptureOptions {
    fn default() -> Self {
        Self {
            annotate: true,
            copy_to_clipboard: true,
            save_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordingTarget {
    Fullscreen,
    Monitor(u32),
    Region(Rect),
    Window(isize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioOptions {
    pub system: bool,
    pub microphone: bool,
}

impl Default for AudioOptions {
    fn default() -> Self {
        Self {
            system: true,
            microphone: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingOptions {
    pub target: RecordingTarget,
    pub output: PathBuf,
    pub audio: AudioOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum WinScreenEvent {
    CaptureDone { path: Option<PathBuf>, size: Size },
    CaptureCanceled,
    RecordingStopped { path: PathBuf },
    PinClosed { id: u64 },
}

pub struct Screenshot;

impl Screenshot {
    pub fn capture_fullscreen() -> Result<CapturedImage> {
        capture::capture_fullscreen()
    }

    pub fn capture_region(rect: Rect) -> Result<CapturedImage> {
        capture::capture_region(rect)
    }

    pub fn capture_monitor(id: u32) -> Result<CapturedImage> {
        capture::capture_monitor(id)
    }

    pub fn capture_window(hwnd: isize) -> Result<CapturedImage> {
        capture::capture_window(hwnd)
    }
}

pub struct Capturer;

impl Capturer {
    pub fn interactive(opts: InteractiveCaptureOptions) -> Result<Option<CapturedImage>> {
        let Some(image) = crate::overlay::interactive_capture()? else {
            return Ok(None);
        };

        if let Some(path) = opts.save_path.as_ref() {
            image.save_png(path)?;
        }

        if opts.copy_to_clipboard {
            image.copy_to_clipboard()?;
        }

        Ok(Some(image))
    }
}

#[derive(Debug, Clone)]
pub struct PinHandle {
    id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinInfo {
    pub id: u64,
    pub size: Size,
}

impl PinHandle {
    pub(crate) fn next() -> Self {
        Self {
            id: NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn close(&self) -> Result<()> {
        pin::close_pin(self.id)
    }

    pub fn set_opacity(&self, opacity: f32) -> Result<()> {
        pin::set_pin_opacity(self.id, opacity)
    }
}

pub struct Pin;

impl Pin {
    pub fn from_image(image: CapturedImage) -> Result<PinHandle> {
        pin::pin_image(image)
    }

    pub fn from_clipboard() -> Result<PinHandle> {
        let image = io::read_clipboard_image()?;
        Self::from_image(image)
    }

    pub fn list() -> Result<Vec<PinInfo>> {
        pin::list_pins()
    }
}

#[derive(Debug)]
pub struct RecordingHandle {
    id: u64,
}

impl RecordingHandle {
    #[allow(dead_code)]
    pub(crate) fn next() -> Self {
        Self {
            id: NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn pause(&self) -> Result<()> {
        record::pause_recording(self.id)
    }

    pub fn resume(&self) -> Result<()> {
        record::resume_recording(self.id)
    }

    pub fn stop(self) -> Result<PathBuf> {
        record::stop_recording(self.id)
    }
}

#[derive(Debug, Clone)]
pub struct Recorder {
    options: RecordingOptions,
}

impl Recorder {
    pub fn builder() -> RecorderBuilder {
        RecorderBuilder::default()
    }

    pub fn start(self) -> Result<RecordingHandle> {
        record::start_recording(self.options)
    }
}

#[derive(Debug, Clone)]
pub struct RecorderBuilder {
    target: RecordingTarget,
    output: Option<PathBuf>,
    audio: AudioOptions,
}

impl Default for RecorderBuilder {
    fn default() -> Self {
        Self {
            target: RecordingTarget::Fullscreen,
            output: None,
            audio: AudioOptions::default(),
        }
    }
}

impl RecorderBuilder {
    pub fn target(mut self, target: RecordingTarget) -> Self {
        self.target = target;
        self
    }

    pub fn audio(mut self, audio: AudioOptions) -> Self {
        self.audio = audio;
        self
    }

    pub fn output(mut self, output: impl Into<PathBuf>) -> Self {
        self.output = Some(output.into());
        self
    }

    pub fn build(self) -> Result<Recorder> {
        let output = self.output.ok_or(WinScreenError::NotImplemented {
            feature: "recording output default path",
        })?;
        Ok(Recorder {
            options: RecordingOptions {
                target: self.target,
                output,
                audio: self.audio,
            },
        })
    }

    pub fn start(self) -> Result<RecordingHandle> {
        self.build()?.start()
    }
}
