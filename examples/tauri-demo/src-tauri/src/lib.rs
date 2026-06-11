#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(win_screen_tauri::init())
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
