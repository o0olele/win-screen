use std::{
    collections::HashMap,
    mem::size_of,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
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
use crate::{RecordingOptions, RecordingTarget, Rect, Result, WinScreenError};

use super::wasapi_audio::{SharedEncoder, WasapiAudio, TARGET_CHANNELS, TARGET_SAMPLE_RATE};

// ─── Shared state ─────────────────────────────────────────────────────────────

struct CaptureShared {
    paused: AtomicBool,
}

// Flags passed into the WGC handler via Context.
pub struct CaptureFlags {
    shared: Arc<CaptureShared>,
    encoder: SharedEncoder,
    // When Some, each frame is cropped before encoding.
    // (start_x, start_y, end_x, end_y) in monitor-local pixel coordinates.
    crop: Option<(u32, u32, u32, u32)>,
}

// ─── WGC capture handler ──────────────────────────────────────────────────────

pub struct ScreenCapture {
    shared: Arc<CaptureShared>,
    encoder: SharedEncoder,
    crop: Option<(u32, u32, u32, u32)>,
    // Reusable scratch buffer for as_nopadding_buffer (avoids per-frame allocation).
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for ScreenCapture {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            shared: ctx.flags.shared,
            encoder: ctx.flags.encoder,
            crop: ctx.flags.crop,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        if self.shared.paused.load(Ordering::Relaxed) {
            return Ok(());
        }

        if let Some(enc) = self.encoder.lock().unwrap().as_mut() {
            if let Some((sx, sy, ex, ey)) = self.crop {
                // Region crop: extract the requested rectangle from the monitor surface.
                let ts = frame.timestamp().map(|t| t.Duration).unwrap_or(0);
                let fb = frame.buffer_crop(sx, sy, ex, ey)?;
                let data = fb.as_nopadding_buffer(&mut self.scratch);

                // D3D staging textures are top-down, but Media Foundation's raw BGRA8
                // CPU buffer path expects bottom-up rows (same convention as DIBs).
                // Flip the rows vertically before encoding.
                let w = (ex - sx) as usize;
                let h = (ey - sy) as usize;
                let row_bytes = w * 4;
                let mut flipped = vec![0u8; data.len()];
                for row in 0..h {
                    let src = row * row_bytes;
                    let dst = (h - 1 - row) * row_bytes;
                    flipped[dst..dst + row_bytes].copy_from_slice(&data[src..src + row_bytes]);
                }

                enc.send_frame_buffer(&flipped, ts)?;
            } else {
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

// ─── Monitor helpers ──────────────────────────────────────────────────────────

// Returns the MONITORINFO for a Monitor.
fn monitor_info(m: &Monitor) -> Option<MONITORINFO> {
    let mut mi = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        GetMonitorInfoW(HMONITOR(m.as_raw_hmonitor()), &mut mi)
    };
    if ok.as_bool() { Some(mi) } else { None }
}

// Area of the intersection of two (l, t, r, b) rectangles; 0 if disjoint.
fn overlap_area(ax1: i32, ay1: i32, ax2: i32, ay2: i32,
                bx1: i32, by1: i32, bx2: i32, by2: i32) -> i64 {
    let ix = (ax2.min(bx2) - ax1.max(bx1)).max(0);
    let iy = (ay2.min(by2) - ay1.max(by1)).max(0);
    ix as i64 * iy as i64
}

// Find the monitor that has the most overlap with `rect` and return the
// Monitor plus crop coordinates in monitor-local space.
fn find_monitor_for_region(rect: &Rect) -> Result<(Monitor, (u32, u32, u32, u32), u32, u32)> {
    let monitors = Monitor::enumerate().map_err(|_| WinScreenError::NotImplemented {
        feature: "monitor enumeration failed",
    })?;

    let rx1 = rect.x;
    let ry1 = rect.y;
    let rx2 = rect.x + rect.width as i32;
    let ry2 = rect.y + rect.height as i32;

    let mut best: Option<(Monitor, i64, MONITORINFO)> = None;

    for m in monitors {
        if let Some(mi) = monitor_info(&m) {
            let mr = mi.rcMonitor;
            let area = overlap_area(rx1, ry1, rx2, ry2, mr.left, mr.top, mr.right, mr.bottom);
            if area > best.as_ref().map_or(0, |(_, a, _)| *a) {
                best = Some((m, area, mi));
            }
        }
    }

    let (monitor, _, mi) = best.ok_or(WinScreenError::NotImplemented {
        feature: "no monitor intersects the requested region",
    })?;

    let mr = mi.rcMonitor;
    let mx = mr.left;
    let my = mr.top;
    let mw = (mr.right - mx) as u32;
    let mh = (mr.bottom - my) as u32;

    // Crop in monitor-local coordinates, clamped to monitor bounds.
    let sx = (rx1 - mx).max(0) as u32;
    let sy = (ry1 - my).max(0) as u32;
    let ex = ((rx2 - mx) as u32).min(mw);
    let ey = ((ry2 - my) as u32).min(mh);

    let crop_w = ex.saturating_sub(sx);
    let crop_h = ey.saturating_sub(sy);

    if crop_w == 0 || crop_h == 0 {
        return Err(WinScreenError::NotImplemented {
            feature: "region does not intersect any monitor",
        });
    }

    Ok((monitor, (sx, sy, ex, ey), crop_w, crop_h))
}

// ─── Start ────────────────────────────────────────────────────────────────────

pub fn start(_id: u64, options: RecordingOptions) -> Result<RecordingEntry> {
    // Resolve monitor, encoder dimensions, and optional crop.
    let (monitor, enc_width, enc_height, crop) = match &options.target {
        RecordingTarget::Fullscreen => {
            let m = Monitor::primary().map_err(|_| WinScreenError::NotImplemented {
                feature: "primary monitor not found",
            })?;
            let w = m.width().map_err(|_| WinScreenError::NotImplemented { feature: "monitor width query failed" })?;
            let h = m.height().map_err(|_| WinScreenError::NotImplemented { feature: "monitor height query failed" })?;
            (m, w, h, None)
        }
        RecordingTarget::Monitor(idx) => {
            let m = Monitor::from_index((*idx as usize) + 1).map_err(|_| WinScreenError::NotImplemented {
                feature: "monitor index out of range",
            })?;
            let w = m.width().map_err(|_| WinScreenError::NotImplemented { feature: "monitor width query failed" })?;
            let h = m.height().map_err(|_| WinScreenError::NotImplemented { feature: "monitor height query failed" })?;
            (m, w, h, None)
        }
        RecordingTarget::Region(rect) => {
            let (m, crop_coords, cw, ch) = find_monitor_for_region(rect)?;
            (m, cw, ch, Some(crop_coords))
        }
        _ => {
            return Err(WinScreenError::NotImplemented {
                feature: "recording target (Fullscreen, Monitor, and Region are supported)",
            });
        }
    };

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
        VideoSettingsBuilder::new(enc_width, enc_height).sub_type(VideoSettingsSubType::H264),
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
        crop,
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
