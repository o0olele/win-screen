import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type Rect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type SelectionResponse = {
  rect: Rect;
  width: number;
  height: number;
  base64Png?: string | null;
  pinned: boolean;
  pinId?: number | null;
};

type PinInfo = {
  id: number;
  size: { width: number; height: number };
  position: Rect;
  displaySize: { width: number; height: number };
  opacity: number;
};

type RawPinInfo = Partial<PinInfo> & {
  display_size?: { width?: number; height?: number } | null;
  displaySize?: { width?: number; height?: number } | null;
  size?: { width?: number; height?: number } | null;
  position?: Partial<Rect> | null;
};

const root = document.querySelector<HTMLDivElement>("#app");
if (!root) {
  throw new Error("missing #app");
}
const app: HTMLDivElement = root;

let currentSelection: SelectionResponse | null = null;
let pins: PinInfo[] = [];
let busy = false;
let message = "Ready";

function plugin<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

async function run(action: () => Promise<void>) {
  busy = true;
  render();
  try {
    await action();
  } catch (error) {
    message = error instanceof Error ? error.message : String(error);
  } finally {
    busy = false;
    render();
  }
}

async function selectRegion() {
  await run(async () => {
    message = "Selecting region...";
    render();
    await plugin<void>("start_interactive_capture_flow", {
      inlineBase64: true,
    });
    message = "Select a region, then use the floating toolbar";
  });
}

async function pinSelection() {
  if (!currentSelection?.base64Png) {
    message = "Select a region first";
    render();
    return;
  }

  await run(async () => {
    const pin = await plugin<{ id: number }>("pin_image", {
      options: { base64Image: currentSelection?.base64Png },
    });
    message = `Created pin ${pin.id}`;
    await refreshPins();
  });
}

async function copySelection() {
  if (!currentSelection?.base64Png) {
    message = "Select a region first";
    render();
    return;
  }

  await run(async () => {
    const pin = await plugin<{ id: number }>("pin_image", {
      options: { base64Image: currentSelection?.base64Png },
    });
    await plugin<void>("copy_pin", { id: pin.id });
    await plugin<void>("close_pin", { id: pin.id });
    message = "Copied selected image via temporary pin";
    await refreshPins();
  });
}

function numberOr(value: unknown, fallback: number) {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function readSize(
  value: { width?: number; height?: number } | null | undefined,
  fallback = { width: 0, height: 0 },
) {
  return {
    width: numberOr(value?.width, fallback.width),
    height: numberOr(value?.height, fallback.height),
  };
}

function readRect(value: RawPinInfo["position"]) {
  return {
    x: numberOr(value?.x, 0),
    y: numberOr(value?.y, 0),
    width: numberOr(value?.width, 0),
    height: numberOr(value?.height, 0),
  };
}

function normalizePin(pin: RawPinInfo | null | undefined): PinInfo | null {
  if (!pin || typeof pin.id !== "number") {
    return null;
  }

  const size = readSize(pin.size);
  const displaySize = readSize(pin.displaySize ?? pin.display_size ?? pin.size, size);
  return {
    id: pin.id,
    size,
    displaySize,
    position: readRect(pin.position),
    opacity: numberOr(pin.opacity, 1),
  };
}

async function refreshPins() {
  const rawPins = await plugin<RawPinInfo[]>("list_pins");
  pins = rawPins.map(normalizePin).filter((pin): pin is PinInfo => pin !== null);
}

async function setPinOpacity(id: number, opacity: number) {
  await run(async () => {
    await plugin<void>("set_pin_opacity", { options: { id, opacity } });
    message = `Set pin ${id} opacity to ${Math.round(opacity * 100)}%`;
    await refreshPins();
  });
}

async function closePin(id: number) {
  await run(async () => {
    await plugin<void>("close_pin", { id });
    message = `Closed pin ${id}`;
    await refreshPins();
  });
}

async function loadPins() {
  await run(async () => {
    await refreshPins();
    message = `Loaded ${pins.length} pin${pins.length === 1 ? "" : "s"}`;
  });
}

function previewHtml() {
  if (!currentSelection?.base64Png) {
    return `<div class="empty">No selection</div>`;
  }

  return `
    <div class="preview-toolbar">
      <button data-action="pin-selection">Pin</button>
      <button data-action="copy-selection">Copy</button>
      <button data-action="clear-selection">Clear</button>
    </div>
    <img class="preview-image" src="data:image/png;base64,${currentSelection.base64Png}" alt="Selected capture" />
    <div class="meta">
      ${currentSelection.width}x${currentSelection.height}
      · screen ${currentSelection.rect.x}, ${currentSelection.rect.y}
    </div>
  `;
}

function pinsHtml() {
  if (pins.length === 0) {
    return `<div class="empty compact">No active pins</div>`;
  }

  return pins
    .map((pin) => {
      return `
        <div class="pin-row">
          <div>
            <strong>#${pin.id}</strong>
            <span>${pin.displaySize.width}x${pin.displaySize.height}</span>
            <span>${pin.position.x}, ${pin.position.y}</span>
          </div>
          <div class="pin-actions">
            <button data-action="opacity-100" data-id="${pin.id}">100%</button>
            <button data-action="opacity-70" data-id="${pin.id}">70%</button>
            <button data-action="close-pin" data-id="${pin.id}">Close</button>
          </div>
        </div>
      `;
    })
    .join("");
}

function render() {
  app.innerHTML = `
    <main>
      <header>
        <div>
          <h1>win-screen Tauri Demo</h1>
          <p>Native overlay selection with a floating WebView toolbar.</p>
        </div>
        <button data-action="select-region" ${busy ? "disabled" : ""}>Select Region</button>
      </header>

      <section class="grid">
        <article class="panel preview">
          <h2>Selection</h2>
          ${previewHtml()}
        </article>

        <article class="panel">
          <div class="panel-heading">
            <h2>Pins</h2>
            <button data-action="refresh-pins" ${busy ? "disabled" : ""}>Refresh</button>
          </div>
          ${pinsHtml()}
        </article>
      </section>

      <footer>${message}</footer>
    </main>
  `;
}

app.addEventListener("click", (event) => {
  const target = event.target as HTMLElement;
  const button = target.closest<HTMLButtonElement>("button[data-action]");
  if (!button || busy) {
    return;
  }

  const action = button.dataset.action;
  const id = Number(button.dataset.id);
  if (action === "select-region") void selectRegion();
  if (action === "pin-selection") void pinSelection();
  if (action === "copy-selection") void copySelection();
  if (action === "clear-selection") {
    currentSelection = null;
    message = "Selection cleared";
    render();
  }
  if (action === "refresh-pins") void loadPins();
  if (action === "opacity-100") void setPinOpacity(id, 1);
  if (action === "opacity-70") void setPinOpacity(id, 0.7);
  if (action === "close-pin") void closePin(id);
});

void loadPins();
void listen<SelectionResponse>("win-screen-demo://selection-done", async (event) => {
  currentSelection = event.payload;
  const pinned = event.payload.pinned && event.payload.pinId ? `, pinned #${event.payload.pinId}` : "";
  message = `Selected ${event.payload.width}x${event.payload.height}${pinned}`;
  await refreshPins();
  render();
});
void listen("win-screen-demo://selection-canceled", () => {
  message = "Selection canceled";
  render();
});
render();
