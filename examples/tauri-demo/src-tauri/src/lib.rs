use base64::Engine;
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use win_screen_core::overlay::{InteractiveOverlay, SelectionDecision};
use win_screen_core::{AudioOptions, CapturedImage, Pin, Rect, RecordingTarget, Recorder, RegionIndicator, Screenshot, Size};

const TOOLBAR_LABEL: &str = "capture-toolbar";
// Toolbar size in physical pixels — the overlay punches a COLOR_KEY hole of the
// same size, so we drive the Tauri window with PhysicalSize to stay aligned.
const TOOLBAR_W: u32 = 240;
const TOOLBAR_H: u32 = 48;

const EVENT_SELECTION_DONE: &str = "win-screen-demo://selection-done";
const EVENT_SELECTION_CANCELED: &str = "win-screen-demo://selection-canceled";
const EVENT_RECORDING_STOPPED: &str = "win-screen-demo://recording-stopped";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveSelectionResponse {
    pub rect: Rect,
    pub width: u32,
    pub height: u32,
    pub base64_png: Option<String>,
    pub pinned: bool,
    pub pin_id: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinResponse {
    pub id: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResponse {
    pub width: u32,
    pub height: u32,
    pub base64_png: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoMonitorInfo {
    pub id: u32,
    pub rect: Rect,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoPinInfo {
    pub id: u64,
    pub size: Size,
    pub position: Rect,
    pub display_size: Size,
    pub opacity: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinImageOptions {
    pub path: Option<PathBuf>,
    pub base64_image: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinOpacityOptions {
    pub id: u64,
    pub opacity: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolbarDecisionOptions {
    pub action: String,
}

#[tauri::command]
fn start_interactive_capture_flow(
    app: AppHandle,
    inline_base64: Option<bool>,
) -> Result<(), String> {
    if capture_running().swap(true, Ordering::SeqCst) {
        return Err("capture flow is already running".to_string());
    }

    thread::Builder::new()
        .name("win-screen-demo-capture-flow".to_string())
        .spawn(move || {
            let _guard = CaptureRunningGuard;

            let app_place = app.clone();
            let app_hide = app.clone();
            let overlay = InteractiveOverlay {
                toolbar_size: (TOOLBAR_W, TOOLBAR_H),
                place_toolbar: Box::new(move |rect: Rect| {
                    if let Some(win) = ensure_toolbar(&app_place) {
                        let _ = win.set_position(PhysicalPosition::new(rect.x, rect.y));
                        let _ = win.set_size(PhysicalSize::new(rect.width, rect.height));
                        let _ = win.show();
                    }
                }),
                hide_toolbar: Box::new(move || {
                    if let Some(win) = app_hide.get_webview_window(TOOLBAR_LABEL) {
                        let _ = win.hide();
                    }
                }),
                on_ready: Box::new(|hwnd: usize| {
                    *overlay_hwnd().lock().unwrap() = Some(hwnd);
                }),
            };

            let result = win_screen_core::overlay::interactive_capture_selection_with_overlay(overlay);

            // The flow is over — hide the toolbar and forget the overlay handle.
            if let Some(win) = app.get_webview_window(TOOLBAR_LABEL) {
                let _ = win.hide();
            }
            *overlay_hwnd().lock().unwrap() = None;

            let Ok(Some((rect, image, decision))) = result else {
                let _ = app.emit(EVENT_SELECTION_CANCELED, ());
                return;
            };

            let mut pinned = false;
            let mut pin_id = None;
            if matches!(decision, SelectionDecision::Pin) {
                if let Ok(handle) = Pin::from_image(image.clone()) {
                    pinned = true;
                    pin_id = Some(handle.id());
                }
            }

            let base64_png = if inline_base64.unwrap_or(true) {
                encode_png_base64(&image).ok()
            } else {
                None
            };

            let payload = InteractiveSelectionResponse {
                rect,
                width: image.width,
                height: image.height,
                base64_png,
                pinned,
                pin_id,
            };
            let _ = app.emit(EVENT_SELECTION_DONE, payload);
        })
        .map_err(|err| {
            capture_running().store(false, Ordering::SeqCst);
            err.to_string()
        })?;

    Ok(())
}

#[tauri::command]
fn pin_image(options: PinImageOptions) -> Result<PinResponse, String> {
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
fn list_pins() -> Result<Vec<DemoPinInfo>, String> {
    Pin::list()
        .map(|pins| {
            pins.into_iter()
                .map(|pin| DemoPinInfo {
                    id: pin.id,
                    size: pin.size,
                    position: pin.position,
                    display_size: pin.display_size,
                    opacity: pin.opacity,
                })
                .collect()
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn copy_pin(id: u64) -> Result<(), String> {
    Pin::copy(id).map_err(|err| err.to_string())
}

#[tauri::command]
fn close_pin(id: u64) -> Result<(), String> {
    win_screen_core::pin::close_pin(id).map_err(|err| err.to_string())
}

#[tauri::command]
fn set_pin_opacity(options: PinOpacityOptions) -> Result<(), String> {
    win_screen_core::pin::set_pin_opacity(options.id, options.opacity)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn capture_fullscreen_demo(clipboard: Option<bool>, inline_base64: Option<bool>) -> Result<CaptureResponse, String> {
    let image = Screenshot::capture_fullscreen().map_err(|e| e.to_string())?;
    if clipboard.unwrap_or(false) {
        image.copy_to_clipboard().map_err(|e| e.to_string())?;
    }
    let base64_png = if inline_base64.unwrap_or(true) {
        Some(encode_png_base64(&image)?)
    } else {
        None
    };
    Ok(CaptureResponse { width: image.width, height: image.height, base64_png })
}

#[tauri::command]
fn capture_monitor_demo(monitor: u32, clipboard: Option<bool>, inline_base64: Option<bool>) -> Result<CaptureResponse, String> {
    let image = Screenshot::capture_monitor(monitor).map_err(|e| e.to_string())?;
    if clipboard.unwrap_or(false) {
        image.copy_to_clipboard().map_err(|e| e.to_string())?;
    }
    let base64_png = if inline_base64.unwrap_or(true) {
        Some(encode_png_base64(&image)?)
    } else {
        None
    };
    Ok(CaptureResponse { width: image.width, height: image.height, base64_png })
}

#[tauri::command]
fn list_monitors_demo() -> Result<Vec<DemoMonitorInfo>, String> {
    Screenshot::monitors()
        .map(|mons| {
            mons.into_iter()
                .map(|m| DemoMonitorInfo { id: m.id, rect: m.rect, primary: m.primary })
                .collect()
        })
        .map_err(|e| e.to_string())
}

fn region_indicator() -> &'static Mutex<Option<RegionIndicator>> {
    static INDICATOR: OnceLock<Mutex<Option<RegionIndicator>>> = OnceLock::new();
    INDICATOR.get_or_init(|| Mutex::new(None))
}

#[tauri::command]
fn show_region_indicator(rect: [i32; 4]) -> Result<(), String> {
    let [x, y, w, h] = rect;
    let r = Rect::new(x, y, w as u32, h as u32).map_err(|e| e.to_string())?;
    let indicator = RegionIndicator::new(r).map_err(|e| e.to_string())?;
    *region_indicator().lock().unwrap() = Some(indicator);
    Ok(())
}

#[tauri::command]
fn hide_region_indicator() {
    *region_indicator().lock().unwrap() = None;
}

#[tauri::command]
fn select_record_region() -> Result<Option<[i32; 4]>, String> {
    if capture_running().swap(true, Ordering::SeqCst) {
        return Err("capture flow is already running".to_string());
    }
    let _guard = CaptureRunningGuard;
    match win_screen_core::overlay::interactive_capture_selection() {
        Ok(Some((rect, _image))) => Ok(Some([rect.x, rect.y, rect.width as i32, rect.height as i32])),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn start_recording_demo(
    output: String,
    system_audio: Option<bool>,
    microphone: Option<bool>,
    monitor: Option<u32>,
    region: Option<[i32; 4]>,
) -> Result<u64, String> {
    let target = if let Some([x, y, w, h]) = region {
        let rect = Rect::new(x, y, w as u32, h as u32).map_err(|e| e.to_string())?;
        RecordingTarget::Region(rect)
    } else {
        match monitor {
            Some(id) => RecordingTarget::Monitor(id),
            None => RecordingTarget::Fullscreen,
        }
    };
    let handle = Recorder::builder()
        .target(target)
        .audio(AudioOptions {
            system: system_audio.unwrap_or(true),
            microphone: microphone.unwrap_or(false),
        })
        .output(PathBuf::from(output))
        .start()
        .map_err(|e| e.to_string())?;
    Ok(handle.id())
}

#[tauri::command]
fn stop_recording_demo(app: AppHandle, id: u64) -> Result<String, String> {
    *region_indicator().lock().unwrap() = None; // hide indicator before stopping
    let path = win_screen_core::record::stop_recording(id).map_err(|e| e.to_string())?;
    let path_str = path.to_string_lossy().into_owned();
    let _ = app.emit(EVENT_RECORDING_STOPPED, &path_str);
    Ok(path_str)
}

#[tauri::command]
fn toolbar_decide(options: ToolbarDecisionOptions) -> Result<(), String> {
    let action = match options.action.as_str() {
        "pin" => SelectionDecision::Pin,
        "cancel" => SelectionDecision::Cancel,
        _ => SelectionDecision::Confirm,
    };

    if let Some(hwnd) = *overlay_hwnd().lock().expect("overlay hwnd poisoned") {
        win_screen_core::overlay::post_decision(hwnd, action);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(win_screen_tauri::init())
        .invoke_handler(tauri::generate_handler![
            start_interactive_capture_flow,
            pin_image,
            list_pins,
            copy_pin,
            close_pin,
            set_pin_opacity,
            toolbar_decide,
            capture_fullscreen_demo,
            capture_monitor_demo,
            list_monitors_demo,
            show_region_indicator,
            hide_region_indicator,
            select_record_region,
            start_recording_demo,
            stop_recording_demo,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}

fn encode_png_base64(image: &CapturedImage) -> Result<String, String> {
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

/// HWND of the currently running selection overlay, so `toolbar_decide` can post
/// the decision straight back into its message loop.
fn overlay_hwnd() -> &'static Mutex<Option<usize>> {
    static HWND: OnceLock<Mutex<Option<usize>>> = OnceLock::new();
    HWND.get_or_init(|| Mutex::new(None))
}

/// Return the capture toolbar window, creating it (hidden) if it doesn't exist.
fn ensure_toolbar(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(win) = app.get_webview_window(TOOLBAR_LABEL) {
        return Some(win);
    }
    WebviewWindowBuilder::new(app, TOOLBAR_LABEL, WebviewUrl::App("toolbar.html".into()))
        .title("Capture toolbar")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .inner_size(TOOLBAR_W as f64, TOOLBAR_H as f64)
        .build()
        .ok()
}

fn capture_running() -> &'static AtomicBool {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    &RUNNING
}

struct CaptureRunningGuard;

impl Drop for CaptureRunningGuard {
    fn drop(&mut self) {
        capture_running().store(false, Ordering::SeqCst);
    }
}

