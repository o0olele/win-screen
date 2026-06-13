use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};
use crate::{RecordingOptions, RecordingTarget, Result, WinScreenError};

use super::wasapi_audio::{SharedEncoder, WasapiAudio, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

// ─── Shared state ─────────────────────────────────────────────────────────────

struct CaptureShared {
    paused: AtomicBool,
}

// Flags passed into the WGC handler via Context.
pub struct CaptureFlags {
    shared: Arc<CaptureShared>,
    encoder: SharedEncoder,
}

// ─── WGC capture handler ──────────────────────────────────────────────────────

pub struct ScreenCapture {
    shared: Arc<CaptureShared>,
    encoder: SharedEncoder,
}

impl GraphicsCaptureApiHandler for ScreenCapture {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            shared: ctx.flags.shared,
            encoder: ctx.flags.encoder,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        if !self.shared.paused.load(Ordering::Relaxed) {
            if let Some(enc) = self.encoder.lock().unwrap().as_mut() {
                enc.send_frame(frame)?;
            }
        }
        Ok(())
    }
}

// ─── Registry ─────────────────────────────────────────────────────────────────

pub struct RecordingEntry {
    shared: Arc<CaptureShared>,
    stop_tx: mpsc::Sender<()>,
    done_rx: mpsc::Receiver<std::result::Result<(), String>>,
    output: PathBuf,
    audio: Option<WasapiAudio>,
}

fn registry() -> &'static Mutex<HashMap<u64, RecordingEntry>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, RecordingEntry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(id: u64, entry: RecordingEntry) {
    registry().lock().unwrap().insert(id, entry);
}

// ─── Start ────────────────────────────────────────────────────────────────────

pub fn start(_id: u64, options: RecordingOptions) -> Result<RecordingEntry> {
    let monitor = match &options.target {
        RecordingTarget::Fullscreen => Monitor::primary().map_err(|_| WinScreenError::NotImplemented {
            feature: "primary monitor not found",
        })?,
        RecordingTarget::Monitor(idx) => {
            Monitor::from_index((*idx as usize) + 1).map_err(|_| WinScreenError::NotImplemented {
                feature: "monitor index out of range",
            })?
        }
        _ => {
            return Err(WinScreenError::NotImplemented {
                feature: "recording target (only Fullscreen and Monitor supported)",
            });
        }
    };

    let width = monitor.width().map_err(|_| WinScreenError::NotImplemented {
        feature: "monitor width query failed",
    })?;
    let height = monitor.height().map_err(|_| WinScreenError::NotImplemented {
        feature: "monitor height query failed",
    })?;

    let audio_enabled = options.audio.system || options.audio.microphone;
    let audio_settings = if audio_enabled {
        AudioSettingsBuilder::new()
            .sample_rate(TARGET_SAMPLE_RATE)
            .channel_count(TARGET_CHANNELS)
            .bit_per_sample(16)
    } else {
        AudioSettingsBuilder::new().disabled(true)
    };

    let encoder = VideoEncoder::new(
        VideoSettingsBuilder::new(width, height).sub_type(VideoSettingsSubType::H264),
        audio_settings,
        ContainerSettingsBuilder::new(),
        &options.output,
    )
    .map_err(|_| WinScreenError::NotImplemented { feature: "VideoEncoder creation failed" })?;

    let encoder: SharedEncoder = Arc::new(Mutex::new(Some(encoder)));
    let shared = Arc::new(CaptureShared { paused: AtomicBool::new(false) });
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel::<std::result::Result<(), String>>();

    let flags = CaptureFlags {
        shared: shared.clone(),
        encoder: encoder.clone(),
    };

    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    );

    let control = ScreenCapture::start_free_threaded(settings)
        .map_err(|_| WinScreenError::NotImplemented { feature: "WGC capture start failed" })?;

    let handler_arc = control.callback();

    // Stop coordinator: waits for signal, halts WGC, then finalises encoder.
    // Audio threads are already stopped before this signal is sent (see stop()).
    std::thread::spawn(move || {
        let _ = stop_rx.recv();
        let _ = control.stop();
        // Clone the encoder Arc while holding the handler lock, then release the lock
        // before calling finish() to avoid holding two nested MutexGuards.
        let encoder_arc = handler_arc.lock().encoder.clone();
        let result = match encoder_arc.lock().unwrap().take() {
            Some(enc) => enc.finish().map_err(|e| e.to_string()),
            None => Ok(()),
        };
        done_tx.send(result).ok();
    });

    // Start WASAPI audio capture (no-op if audio disabled).
    let audio = if audio_enabled {
        Some(WasapiAudio::start(encoder, options.audio.system, options.audio.microphone))
    } else {
        None
    };

    Ok(RecordingEntry {
        shared,
        stop_tx,
        done_rx,
        output: options.output,
        audio,
    })
}

// ─── Pause / Resume / Stop ────────────────────────────────────────────────────

pub fn pause(id: u64) -> Result<()> {
    let guard = registry().lock().unwrap();
    guard
        .get(&id)
        .ok_or(WinScreenError::NotImplemented { feature: "recording id not found (pause)" })
        .map(|entry| entry.shared.paused.store(true, Ordering::Relaxed))
}

pub fn resume(id: u64) -> Result<()> {
    let guard = registry().lock().unwrap();
    guard
        .get(&id)
        .ok_or(WinScreenError::NotImplemented { feature: "recording id not found (resume)" })
        .map(|entry| entry.shared.paused.store(false, Ordering::Relaxed))
}

pub fn stop(id: u64) -> Result<PathBuf> {
    let entry = registry()
        .lock()
        .unwrap()
        .remove(&id)
        .ok_or(WinScreenError::NotImplemented { feature: "recording id not found (stop)" })?;

    // Stop audio capture threads first (no more send_audio_buffer after this).
    if let Some(audio) = entry.audio {
        audio.stop();
    }

    // Signal the video stop coordinator.
    let _ = entry.stop_tx.send(());

    // Wait for the encoder to be finalised.
    entry
        .done_rx
        .recv()
        .map_err(|_| WinScreenError::NotImplemented { feature: "recording channel closed unexpectedly" })?
        .map_err(|_| WinScreenError::NotImplemented { feature: "recording encoder finalisation failed" })?;

    Ok(entry.output)
}
