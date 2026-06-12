# win-screen Tauri Demo

Minimal integration sketch for the `win-screen-tauri` plugin.

Register the plugin in a Tauri v2 application:

```rust
tauri::Builder::default()
    .plugin(win_screen_tauri::init())
    .run(tauri::generate_context!())
    .expect("failed to run tauri app");
```

Frontend commands:

```ts
import { invoke } from "@tauri-apps/api/core";

const capture = await invoke("plugin:win-screen|capture_fullscreen", {
  options: {
    savePath: "capture.png",
    clipboard: true,
    inlineBase64: false,
  },
});
```

Create a pin directly from a capture:

```ts
await invoke("plugin:win-screen|interactive_capture_to_pin");
```

Pin an existing image file and list active native pins:

```ts
await invoke("plugin:win-screen|pin_image", {
  options: { path: "capture.png" },
});

const pins = await invoke("plugin:win-screen|list_pins");

await invoke("plugin:win-screen|set_pin_opacity", {
  options: { id: 1, opacity: 0.75 },
});

await invoke("plugin:win-screen|copy_pin", { id: 1 });
await invoke("plugin:win-screen|save_pin", {
  options: { id: 1, path: "pin.png" },
});
```

Capture a specific monitor or native window:

```ts
await invoke("plugin:win-screen|capture_monitor", {
  options: { monitor: 0, savePath: "monitor.png" },
});

await invoke("plugin:win-screen|capture_window_to_pin", {
  options: { hwnd: 123456 },
});

const monitors = await invoke("plugin:win-screen|list_monitors");
const windows = await invoke("plugin:win-screen|list_windows");
```
