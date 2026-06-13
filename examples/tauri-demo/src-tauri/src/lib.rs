use base64::Engine;
use crossbeam_channel::Sender;
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use win_screen_core::overlay::SelectionDecision;
use win_screen_core::{CapturedImage, Pin, Rect, Size};

const EVENT_SELECTION_DONE: &str = "win-screen-demo://selection-done";
const EVENT_SELECTION_CANCELED: &str = "win-screen-demo://selection-canceled";

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
            let app_for_decision = app.clone();
            let result =
                win_screen_core::overlay::interactive_capture_selection_with_decision(move |rect| {
                    show_toolbar_and_wait(&app_for_decision, rect)
                });

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
fn toolbar_decide(options: ToolbarDecisionOptions) -> Result<(), String> {
    let action = match options.action.as_str() {
        "pin" => SelectionDecision::Pin,
        "cancel" => SelectionDecision::Cancel,
        _ => SelectionDecision::Confirm,
    };

    if let Some(sender) = toolbar_sender().lock().expect("toolbar sender poisoned").take() {
        let _ = sender.send(action);
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
            toolbar_decide
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

fn toolbar_sender() -> &'static Mutex<Option<Sender<SelectionDecision>>> {
    static SENDER: OnceLock<Mutex<Option<Sender<SelectionDecision>>>> = OnceLock::new();
    SENDER.get_or_init(|| Mutex::new(None))
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

fn show_toolbar_and_wait(app: &AppHandle, rect: Rect) -> SelectionDecision {
    let (tx, rx) = crossbeam_channel::bounded(1);
    *toolbar_sender().lock().expect("toolbar sender poisoned") = Some(tx);

    let width = 220.0;
    let height = 44.0;
    let x = rect.x as f64;
    let y = (rect.y + rect.height as i32 + 8) as f64;
    let label = "capture-toolbar";
    let toolbar = if let Some(existing) = app.get_webview_window(label) {
        existing
    } else {
        match WebviewWindowBuilder::new(app, label, WebviewUrl::App("toolbar.html".into()))
            .title("Capture toolbar")
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .inner_size(width, height)
            .position(x, y)
            .build()
        {
            Ok(toolbar) => toolbar,
            Err(_) => {
                *toolbar_sender().lock().expect("toolbar sender poisoned") = None;
                return SelectionDecision::Cancel;
            }
        }
    };
    let _ = toolbar.set_position(PhysicalPosition::new(x as i32, y as i32));
    let _ = toolbar.show();
    let _ = toolbar.set_focus();

    let decision = rx
        .recv_timeout(Duration::from_secs(60))
        .unwrap_or(SelectionDecision::Cancel);
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.hide();
    }
    *toolbar_sender().lock().expect("toolbar sender poisoned") = None;
    decision
}
