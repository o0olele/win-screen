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
use win_screen_core::{
    AnnotationCommand, AnnotationEditAction, AnnotationOverlay, AnnotationTool, AudioOptions,
    CapturedImage, Color, Pin, Rect, Recorder, RecordingTarget, RegionIndicator, Screenshot, Size,
};

const TOOLBAR_LABEL: &str = "capture-toolbar";
// Rich floating toolbar (tools + colors + width + undo/redo + finish), in physical
// pixels. The selection overlay punches a COLOR_KEY hole of the same size, so we
// drive the Tauri window with PhysicalSize to stay aligned.
const TOOLBAR_W: u32 = 600;
const TOOLBAR_H: u32 = 52;

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

            // The overlay already drew any annotations onto `image`. Finish per the
            // toolbar decision: Pin → desktop pin, Save → PNG on the desktop.
            let mut pinned = false;
            let mut pin_id = None;
            if matches!(decision, SelectionDecision::Pin) {
                if let Ok(handle) = Pin::from_image(image.clone()) {
                    pinned = true;
                    pin_id = Some(handle.id());
                }
            }
            if matches!(decision, SelectionDecision::Save) {
                if let Some(path) = desktop_png_path() {
                    let _ = image.save_png(&path);
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

// ─── Annotation flow ────────────────────────────────────────────────────────────

/// HWND of the running annotation editor, so `annotation_command` can post commands
/// (set tool/color/width, undo/redo, confirm/pin/save/cancel) straight into it.
fn annotate_hwnd() -> &'static Mutex<Option<usize>> {
    static HWND: OnceLock<Mutex<Option<usize>>> = OnceLock::new();
    HWND.get_or_init(|| Mutex::new(None))
}

/// Run win-screen-core's annotation editor on `image`. The editor (core) draws the
/// image and handles the mouse; the floating toolbar is the shared Tauri window
/// above. On finish, pin / save / hand the annotated image back to the main window.
/// Used by the preview-area "标注" entry, where there is no live screen region.
///
/// Synchronous — runs the editor message loop on the calling thread.
fn run_annotation_editor(app: AppHandle, image: CapturedImage, anchor: Option<Rect>) {
    let app_place = app.clone();
    let app_hide = app.clone();
    let overlay = AnnotationOverlay {
        toolbar_size: (TOOLBAR_W, TOOLBAR_H),
        place_toolbar: Box::new(move |rect: Rect| {
            if let Some(win) = ensure_toolbar(&app_place) {
                let _ = win.set_position(PhysicalPosition::new(rect.x, rect.y));
                let _ = win.set_size(PhysicalSize::new(rect.width, rect.height));
                let _ = win.show();
                let _ = win.set_focus();
            }
        }),
        hide_toolbar: Box::new(move || {
            if let Some(win) = app_hide.get_webview_window(TOOLBAR_LABEL) {
                let _ = win.hide();
            }
        }),
        on_ready: Box::new(|hwnd: usize| {
            *annotate_hwnd().lock().unwrap() = Some(hwnd);
        }),
    };

    let result = win_screen_core::edit_image_with_overlay(image, anchor, overlay);

    // Editor closed — forget the toolbar and editor handle.
    if let Some(win) = app.get_webview_window(TOOLBAR_LABEL) {
        let _ = win.hide();
    }
    *annotate_hwnd().lock().unwrap() = None;

    let Ok(Some(edit)) = result else {
        let _ = app.emit(EVENT_SELECTION_CANCELED, ());
        return;
    };

    let mut pinned = false;
    let mut pin_id = None;
    if matches!(edit.action, AnnotationEditAction::Pin) {
        if let Ok(handle) = Pin::from_image(edit.image.clone()) {
            pinned = true;
            pin_id = Some(handle.id());
        }
    }
    if matches!(edit.action, AnnotationEditAction::Save) {
        if let Some(path) = desktop_png_path() {
            let _ = edit.image.save_png(&path);
        }
    }

    let base64_png = encode_png_base64(&edit.image).ok();
    let rect = anchor.unwrap_or(Rect {
        x: 0,
        y: 0,
        width: edit.image.width,
        height: edit.image.height,
    });
    let payload = InteractiveSelectionResponse {
        rect,
        width: edit.image.width,
        height: edit.image.height,
        base64_png,
        pinned,
        pin_id,
    };
    let _ = app.emit(EVENT_SELECTION_DONE, payload);
}

/// `~/Desktop/annotation-<unix_ts>.png`, or None if the home dir can't be resolved.
fn desktop_png_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut path = PathBuf::from(home);
    path.push("Desktop");
    path.push(format!("annotation-{ts}.png"));
    Some(path)
}

/// Open the annotation editor for an arbitrary captured image (base64 PNG). Used by
/// the preview-toolbar "标注" button so fullscreen/monitor captures can be annotated.
#[tauri::command]
fn annotate_image_demo(app: AppHandle, base64_image: String) -> Result<(), String> {
    if capture_running().swap(true, Ordering::SeqCst) {
        return Err("a capture or annotation flow is already running".to_string());
    }
    thread::Builder::new()
        .name("win-screen-demo-annotate".to_string())
        .spawn(move || {
            let _guard = CaptureRunningGuard;
            let bytes = match base64::engine::general_purpose::STANDARD.decode(base64_image) {
                Ok(b) => b,
                Err(_) => return,
            };
            let Ok(decoded) = image::load_from_memory(&bytes) else {
                return;
            };
            let rgba = decoded.to_rgba8();
            let Ok(captured) = CapturedImage::new(rgba.width(), rgba.height(), rgba.into_raw())
            else {
                return;
            };
            run_annotation_editor(app, captured, None);
        })
        .map_err(|err| {
            capture_running().store(false, Ordering::SeqCst);
            err.to_string()
        })?;
    Ok(())
}

/// Drive the active annotation surface from the floating Tauri toolbar. Routes to the
/// selection overlay when one is running, otherwise the standalone editor (preview-area
/// entry). `action` is one of: `tool:<rectangle|ellipse|arrow|line|brush|text|mosaic|number>`,
/// `tool:none` (deselect), `color:#RRGGBB`, `width:<n>`, `undo`, `redo`, `clear`,
/// `confirm`, `pin`, `save`, `cancel`.
#[tauri::command]
fn annotation_command(options: ToolbarDecisionOptions) -> Result<(), String> {
    let action = options.action;
    let command = if let Some(tool) = action.strip_prefix("tool:") {
        if tool == "none" {
            AnnotationCommand::DeselectTool
        } else {
            let tool = match tool {
                "rectangle" => AnnotationTool::Rectangle,
                "ellipse" => AnnotationTool::Ellipse,
                "arrow" => AnnotationTool::Arrow,
                "line" => AnnotationTool::Line,
                "brush" => AnnotationTool::Brush,
                "text" => AnnotationTool::Text,
                "mosaic" => AnnotationTool::Mosaic,
                "number" => AnnotationTool::Number,
                other => return Err(format!("unknown tool: {other}")),
            };
            AnnotationCommand::SetTool(tool)
        }
    } else if let Some(hex) = action.strip_prefix("color:") {
        AnnotationCommand::SetColor(parse_hex_color(hex)?)
    } else if let Some(width) = action.strip_prefix("width:") {
        let width: u32 = width.parse().map_err(|_| "invalid width".to_string())?;
        AnnotationCommand::SetStrokeWidth(width)
    } else {
        match action.as_str() {
            "undo" => AnnotationCommand::Undo,
            "redo" => AnnotationCommand::Redo,
            "clear" => AnnotationCommand::Clear,
            "confirm" => AnnotationCommand::Confirm,
            "pin" => AnnotationCommand::Pin,
            "save" => AnnotationCommand::Save,
            "cancel" => AnnotationCommand::Cancel,
            other => return Err(format!("unknown annotation action: {other}")),
        }
    };

    // The selection overlay and the standalone editor are mutually exclusive
    // (capture_running guards both). Prefer the overlay when it is up.
    if let Some(hwnd) = *overlay_hwnd().lock().expect("overlay hwnd poisoned") {
        win_screen_core::post_overlay_command(hwnd, command);
    } else if let Some(hwnd) = *annotate_hwnd().lock().expect("annotate hwnd poisoned") {
        win_screen_core::post_annotation_command(hwnd, command);
    }
    Ok(())
}

/// Parse `#RRGGBB` (or `RRGGBB`) into an opaque Color.
fn parse_hex_color(hex: &str) -> Result<Color, String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Err(format!("invalid color: {hex}"));
    }
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "invalid color".to_string())?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "invalid color".to_string())?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "invalid color".to_string())?;
    Ok(Color { r, g, b, a: 255 })
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
            capture_fullscreen_demo,
            capture_monitor_demo,
            list_monitors_demo,
            show_region_indicator,
            hide_region_indicator,
            select_record_region,
            start_recording_demo,
            stop_recording_demo,
            annotate_image_demo,
            annotation_command,
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

/// HWND of the currently running selection overlay, so `annotation_command` can post
/// tool/finish commands straight back into its message loop.
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

