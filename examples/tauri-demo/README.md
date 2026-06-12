# win-screen Tauri Demo

Manual test app for the `win-screen-tauri` plugin.

The demo uses the intended split flow:

1. Native Win32 overlay selects a region.
2. The plugin returns the selected image and screen rect to the Web UI.
3. The Web UI shows preview controls.
4. Clicking `Pin` calls the native pin command.

## Run

```powershell
cd examples/tauri-demo
npm install
npm run tauri dev
```

## Test Flow

- Click `Select Region`.
- Drag a region in the native overlay.
- The selected image appears in the Tauri Web UI.
- Click `Pin` to create a native desktop pin.
- Use `Refresh`, `100%`, `70%`, and `Close` to test pin state commands.

## Plugin Commands Used

- `select_interactive_capture`
- `pin_image`
- `list_pins`
- `set_pin_opacity`
- `close_pin`
