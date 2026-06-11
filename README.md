# win-screen

Windows-first Rust workspace for screenshots, screen recording, and desktop pinning.

## Crates

- `win-screen-core`: core API and Windows screenshot backend.
- `win-screen-cli`: standalone `win-screen` executable for manual validation.
- `win-screen-tauri`: Tauri v2 plugin wrapper with commands and events.

## Current Status

Implemented in this scaffold:

- Workspace with core, CLI, and Tauri plugin crates.
- Fullscreen, region, monitor, and window screenshot facade.
- Initial Windows GDI screenshot backend returning RGBA buffers.
- PNG/JPEG saving and image clipboard read/write.
- CLI commands: `shot`, `record`, `pin`.
- Tauri commands: `capture_fullscreen`, `capture_region`, `start_interactive_capture`, `start_recording`, `stop_recording`, `pin_from_clipboard`, `close_pin`.

Planned next phases:

- Native Win32 layered overlay and annotation editor.
- Native desktop pin windows.
- WGC/Media Foundation MP4 recording and WASAPI mixing.

## CLI

```powershell
cargo run -p win-screen-cli -- shot --fullscreen --save out.png
cargo run -p win-screen-cli -- shot --region 0 0 800 600 --save region.png --clipboard
```

Interactive capture, pin windows, and recording currently return explicit `not implemented yet` errors.
