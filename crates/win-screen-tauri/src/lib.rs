use base64::Engine;
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tauri::{plugin::TauriPlugin, AppHandle, Emitter, Runtime};
use win_screen_core::{
    AudioOptions, CapturedImage, Capturer, HotkeyHandle, InteractiveCaptureOptions, MonitorInfo,
    Pin, PinInfo, Recorder, RecordingTarget, Rect, Screenshot, WindowInfo,
};

pub const EVENT_CAPTURE_DONE: &str = "win-screen://capture-done";
pub const EVENT_RECORDING_STOPPED: &str = "win-screen://recording-stopped";
pub const EVENT_HOTKEY_FIRED: &str = "win-screen://hotkey-fired";

fn hotkey_registry() -> &'static Mutex<HashMap<i32, HotkeyHandle>> {
    static R: OnceLock<Mutex<HashMap<i32, HotkeyHandle>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResponse {
    pub width: u32,
    pub height: u32,
    pub path: Option<PathBuf>,
    pub base64_png: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveSelectionResponse {
    pub rect: Rect,
    pub width: u32,
    pub height: u32,
    pub base64_png: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOptions {
    pub save_path: Option<PathBuf>,
    pub clipboard: Option<bool>,
    pub inline_base64: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionCaptureOptions {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub save_path: Option<PathBuf>,
    pub clipboard: Option<bool>,
    pub inline_base64: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorCaptureOptions {
    pub monitor: u32,
    pub save_path: Option<PathBuf>,
    pub clipboard: Option<bool>,
    pub inline_base64: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowCaptureOptions {
    pub hwnd: isize,
    pub save_path: Option<PathBuf>,
    pub clipboard: Option<bool>,
    pub inline_base64: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingOptions {
    pub output: PathBuf,
    pub system_audio: Option<bool>,
    pub microphone: Option<bool>,
    /// Record a specific monitor (0-based index). Ignored when `region` is set.
    pub monitor: Option<u32>,
    /// Record a specific region [x, y, width, height] in virtual screen coordinates.
    /// Takes priority over `monitor`.
    pub region: Option<[i32; 4]>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResponse {
    pub id: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinResponse {
    pub id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinImageOptions {
    pub path: Option<PathBuf>,
    pub base64_image: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePinOptions {
    pub id: u64,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinOpacityOptions {
    pub id: u64,
    pub opacity: f32,
}

pub mod commands {
    use super::*;

    #[tauri::command]
    pub fn capture_fullscreen<R: Runtime>(
        app: AppHandle<R>,
        options: Option<CaptureOptions>,
    ) -> Result<CaptureResponse, String> {
        let options = options.unwrap_or(CaptureOptions {
            save_path: None,
            clipboard: Some(false),
            inline_base64: Some(false),
        });
        let image = Screenshot::capture_fullscreen().map_err(|err| err.to_string())?;
        finish_capture(app, image, options)
    }

    #[tauri::command]
    pub fn capture_region<R: Runtime>(
        app: AppHandle<R>,
        options: RegionCaptureOptions,
    ) -> Result<CaptureResponse, String> {
        let rect = Rect::new(options.x, options.y, options.width, options.height)
            .map_err(|err| err.to_string())?;
        let image = Screenshot::capture_region(rect).map_err(|err| err.to_string())?;
        finish_capture(
            app,
            image,
            CaptureOptions {
                save_path: options.save_path,
                clipboard: options.clipboard,
                inline_base64: options.inline_base64,
            },
        )
    }

    #[tauri::command]
    pub fn capture_monitor<R: Runtime>(
        app: AppHandle<R>,
        options: MonitorCaptureOptions,
    ) -> Result<CaptureResponse, String> {
        let image = Screenshot::capture_monitor(options.monitor).map_err(|err| err.to_string())?;
        finish_capture(
            app,
            image,
            CaptureOptions {
                save_path: options.save_path,
                clipboard: options.clipboard,
                inline_base64: options.inline_base64,
            },
        )
    }

    #[tauri::command]
    pub fn capture_window<R: Runtime>(
        app: AppHandle<R>,
        options: WindowCaptureOptions,
    ) -> Result<CaptureResponse, String> {
        let image = Screenshot::capture_window(options.hwnd).map_err(|err| err.to_string())?;
        finish_capture(
            app,
            image,
            CaptureOptions {
                save_path: options.save_path,
                clipboard: options.clipboard,
                inline_base64: options.inline_base64,
            },
        )
    }

    #[tauri::command]
    pub fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
        Screenshot::monitors().map_err(|err| err.to_string())
    }

    #[tauri::command]
    pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
        Screenshot::windows().map_err(|err| err.to_string())
    }

    #[tauri::command]
    pub fn capture_fullscreen_to_pin() -> Result<PinResponse, String> {
        let image = Screenshot::capture_fullscreen().map_err(|err| err.to_string())?;
        let handle = Pin::from_image(image).map_err(|err| err.to_string())?;
        Ok(PinResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn capture_region_to_pin(options: RegionCaptureOptions) -> Result<PinResponse, String> {
        let rect = Rect::new(options.x, options.y, options.width, options.height)
            .map_err(|err| err.to_string())?;
        let image = Screenshot::capture_region(rect).map_err(|err| err.to_string())?;
        let handle = Pin::from_image(image).map_err(|err| err.to_string())?;
        Ok(PinResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn capture_monitor_to_pin(options: MonitorCaptureOptions) -> Result<PinResponse, String> {
        let image = Screenshot::capture_monitor(options.monitor).map_err(|err| err.to_string())?;
        let handle = Pin::from_image(image).map_err(|err| err.to_string())?;
        Ok(PinResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn capture_window_to_pin(options: WindowCaptureOptions) -> Result<PinResponse, String> {
        let image = Screenshot::capture_window(options.hwnd).map_err(|err| err.to_string())?;
        let handle = Pin::from_image(image).map_err(|err| err.to_string())?;
        Ok(PinResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn interactive_capture_to_pin() -> Result<Option<PinResponse>, String> {
        let Some(result) = Capturer::interactive_with_result(InteractiveCaptureOptions {
            annotate: true,
            copy_to_clipboard: false,
            save_path: None,
        })
        .map_err(|err| err.to_string())?
        else {
            return Ok(None);
        };

        if !result.pin_requested {
            return Ok(None);
        }

        let handle = Pin::from_image(result.image).map_err(|err| err.to_string())?;
        Ok(Some(PinResponse { id: handle.id() }))
    }

    #[tauri::command]
    pub fn start_interactive_capture<R: Runtime>(
        app: AppHandle<R>,
        options: Option<CaptureOptions>,
    ) -> Result<Option<CaptureResponse>, String> {
        let options = options.unwrap_or(CaptureOptions {
            save_path: None,
            clipboard: Some(true),
            inline_base64: Some(false),
        });
        let image = Capturer::interactive(InteractiveCaptureOptions {
            annotate: true,
            copy_to_clipboard: options.clipboard.unwrap_or(true),
            save_path: options.save_path.clone(),
        })
        .map_err(|err| err.to_string())?;

        match image {
            Some(image) => finish_capture(
                app,
                image,
                CaptureOptions {
                    clipboard: Some(false),
                    save_path: None,
                    inline_base64: options.inline_base64,
                },
            )
            .map(Some),
            None => Ok(None),
        }
    }

    #[tauri::command]
    pub fn select_interactive_capture(
        inline_base64: Option<bool>,
    ) -> Result<Option<InteractiveSelectionResponse>, String> {
        let Some((rect, image)) = win_screen_core::overlay::interactive_capture_selection()
            .map_err(|err| err.to_string())?
        else {
            return Ok(None);
        };

        let base64_png = if inline_base64.unwrap_or(true) {
            Some(encode_png_base64(&image)?)
        } else {
            None
        };

        Ok(Some(InteractiveSelectionResponse {
            rect,
            width: image.width,
            height: image.height,
            base64_png,
        }))
    }

    #[tauri::command]
    pub fn start_recording(options: RecordingOptions) -> Result<RecordingResponse, String> {
        let target = if let Some(r) = options.region {
            let rect = Rect::new(r[0], r[1], r[2] as u32, r[3] as u32)
                .map_err(|err| err.to_string())?;
            RecordingTarget::Region(rect)
        } else if let Some(idx) = options.monitor {
            RecordingTarget::Monitor(idx)
        } else {
            RecordingTarget::Fullscreen
        };

        let handle = Recorder::builder()
            .target(target)
            .audio(AudioOptions {
                system: options.system_audio.unwrap_or(true),
                microphone: options.microphone.unwrap_or(false),
            })
            .output(options.output)
            .start()
            .map_err(|err| err.to_string())?;

        Ok(RecordingResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn stop_recording<R: Runtime>(app: AppHandle<R>, id: u64) -> Result<PathBuf, String> {
        let path = win_screen_core::record::stop_recording(id).map_err(|err| err.to_string())?;
        let _ = app.emit(EVENT_RECORDING_STOPPED, &path);
        Ok(path)
    }

    #[tauri::command]
    pub fn pin_from_clipboard() -> Result<PinResponse, String> {
        let handle = Pin::from_clipboard().map_err(|err| err.to_string())?;
        Ok(PinResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn pin_image(options: PinImageOptions) -> Result<PinResponse, String> {
        let image = if let Some(path) = options.path {
            CapturedImage::load(path).map_err(|err| err.to_string())?
        } else if let Some(base64_image) = options.base64_image {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(base64_image)
                .map_err(|err| err.to_string())?;
            let image = image::load_from_memory(&bytes)
                .map_err(|err| err.to_string())?
                .to_rgba8();
            CapturedImage::new(image.width(), image.height(), image.into_raw())
                .map_err(|err| err.to_string())?
        } else {
            return Err("pin_image requires path or base64Image".to_string());
        };

        let handle = Pin::from_image(image).map_err(|err| err.to_string())?;
        Ok(PinResponse { id: handle.id() })
    }

    #[tauri::command]
    pub fn list_pins() -> Result<Vec<PinInfo>, String> {
        Pin::list().map_err(|err| err.to_string())
    }

    #[tauri::command]
    pub fn copy_pin(id: u64) -> Result<(), String> {
        Pin::copy(id).map_err(|err| err.to_string())
    }

    #[tauri::command]
    pub fn save_pin(options: SavePinOptions) -> Result<(), String> {
        Pin::save_png(options.id, options.path).map_err(|err| err.to_string())
    }

    #[tauri::command]
    pub fn set_pin_opacity(options: PinOpacityOptions) -> Result<(), String> {
        win_screen_core::pin::set_pin_opacity(options.id, options.opacity)
            .map_err(|err| err.to_string())
    }

    #[tauri::command]
    pub fn close_pin(id: u64) -> Result<(), String> {
        win_screen_core::pin::close_pin(id).map_err(|err| err.to_string())
    }

    /// Register a global hotkey. Emits `win-screen://hotkey-fired` with the
    /// hotkey `id` each time the key combination is pressed.
    ///
    /// `modifiers`: bitwise OR of `hotkey_mod::*` constants (alt=1, ctrl=2,
    /// shift=4, win=8, norepeat=0x4000).
    /// `vk`: Windows virtual-key code (e.g. 0x2C for Print Screen).
    #[tauri::command]
    pub fn register_hotkey<R: Runtime>(
        app: AppHandle<R>,
        id: i32,
        modifiers: u32,
        vk: u32,
    ) -> Result<(), String> {
        let handle = win_screen_core::register_hotkey(id, modifiers, vk, move || {
            let _ = app.emit(super::EVENT_HOTKEY_FIRED, id);
        })
        .map_err(|err| err.to_string())?;

        super::hotkey_registry().lock().unwrap().insert(id, handle);
        Ok(())
    }

    /// Unregister a previously registered global hotkey by its `id`.
    #[tauri::command]
    pub fn unregister_hotkey(id: i32) -> Result<(), String> {
        super::hotkey_registry()
            .lock()
            .unwrap()
            .remove(&id)
            .map(|h| h.unregister())
            .ok_or_else(|| format!("hotkey {id} not registered"))
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("win-screen")
        .invoke_handler(tauri::generate_handler![
            commands::capture_fullscreen,
            commands::capture_region,
            commands::capture_monitor,
            commands::capture_window,
            commands::list_monitors,
            commands::list_windows,
            commands::capture_fullscreen_to_pin,
            commands::capture_region_to_pin,
            commands::capture_monitor_to_pin,
            commands::capture_window_to_pin,
            commands::interactive_capture_to_pin,
            commands::start_interactive_capture,
            commands::select_interactive_capture,
            commands::start_recording,
            commands::stop_recording,
            commands::pin_image,
            commands::pin_from_clipboard,
            commands::list_pins,
            commands::copy_pin,
            commands::save_pin,
            commands::set_pin_opacity,
            commands::close_pin,
            commands::register_hotkey,
            commands::unregister_hotkey,
        ])
        .build()
}

fn finish_capture<R: Runtime>(
    app: AppHandle<R>,
    image: win_screen_core::CapturedImage,
    options: CaptureOptions,
) -> Result<CaptureResponse, String> {
    if options.clipboard.unwrap_or(false) {
        image.copy_to_clipboard().map_err(|err| err.to_string())?;
    }

    if let Some(path) = &options.save_path {
        image.save_png(path).map_err(|err| err.to_string())?;
    }

    let base64_png = if options.inline_base64.unwrap_or(false) {
        Some(encode_png_base64(&image)?)
    } else {
        None
    };

    let response = CaptureResponse {
        width: image.width,
        height: image.height,
        path: options.save_path,
        base64_png,
    };
    let _ = app.emit(EVENT_CAPTURE_DONE, &response);
    Ok(response)
}

fn encode_png_base64(image: &win_screen_core::CapturedImage) -> Result<String, String> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &image.rgba,
            image.width,
            image.height,
            image::ColorType::Rgba8.into(),
        )
        .map_err(|err| err.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}
