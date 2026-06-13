import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

// ─── Types ────────────────────────────────────────────────────────────────────

type Rect = { x: number; y: number; width: number; height: number };

type SelectionResponse = {
  rect: Rect;
  width: number;
  height: number;
  base64Png?: string | null;
  pinned: boolean;
  pinId?: number | null;
};

type CaptureResponse = {
  width: number;
  height: number;
  base64Png?: string | null;
};

type MonitorInfo = {
  id: number;
  rect: Rect;
  primary: boolean;
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

// ─── State ────────────────────────────────────────────────────────────────────

const root = document.querySelector<HTMLDivElement>("#app")!;

type Tab = "capture" | "record" | "pins";

let activeTab: Tab = "capture";
let currentSelection: SelectionResponse | null = null;
let pins: PinInfo[] = [];
let monitors: MonitorInfo[] = [];
let busy = false;
let message = "Ready";

// recording state
type RecordTarget = "fullscreen" | "monitor" | "region";
let recordingId: number | null = null;
let recordingSeconds = 0;
let recordingTimer: ReturnType<typeof setInterval> | null = null;
let recordOutput = "";
let systemAudio = true;
let useMic = false;
let recordTarget: RecordTarget = "fullscreen";
let recordMonitor: number | null = null;
let recordRegion: [number, number, number, number] | null = null;

// ─── Helpers ──────────────────────────────────────────────────────────────────

function invoke_demo<T>(command: string, args?: Record<string, unknown>): Promise<T> {
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

function numberOr(v: unknown, fallback: number) {
  return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

function normalizePin(pin: RawPinInfo | null | undefined): PinInfo | null {
  if (!pin || typeof pin.id !== "number") return null;
  const size = { width: numberOr(pin.size?.width, 0), height: numberOr(pin.size?.height, 0) };
  const displaySize = {
    width: numberOr((pin.displaySize ?? pin.display_size ?? pin.size)?.width, size.width),
    height: numberOr((pin.displaySize ?? pin.display_size ?? pin.size)?.height, size.height),
  };
  return {
    id: pin.id,
    size,
    displaySize,
    position: {
      x: numberOr(pin.position?.x, 0),
      y: numberOr(pin.position?.y, 0),
      width: numberOr(pin.position?.width, 0),
      height: numberOr(pin.position?.height, 0),
    },
    opacity: numberOr(pin.opacity, 1),
  };
}

function fmtTime(seconds: number): string {
  const m = Math.floor(seconds / 60).toString().padStart(2, "0");
  const s = (seconds % 60).toString().padStart(2, "0");
  return `${m}:${s}`;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
}

// ─── Data loaders ─────────────────────────────────────────────────────────────

async function refreshPins() {
  const rawPins = await invoke_demo<RawPinInfo[]>("list_pins");
  pins = rawPins.map(normalizePin).filter((p): p is PinInfo => p !== null);
}

async function loadPins() {
  await run(async () => {
    await refreshPins();
    message = `${pins.length} pin${pins.length === 1 ? "" : "s"} loaded`;
  });
}

async function loadMonitors() {
  try {
    monitors = await invoke_demo<MonitorInfo[]>("list_monitors_demo");
  } catch {
    monitors = [];
  }
}

// ─── Capture tab actions ──────────────────────────────────────────────────────

async function selectRegion() {
  await run(async () => {
    message = "Selecting region…";
    render();
    await invoke_demo<void>("start_interactive_capture_flow", { inlineBase64: true });
    message = "Select a region, then use the floating toolbar";
  });
}

async function captureFullscreen() {
  await run(async () => {
    message = "Capturing fullscreen…";
    render();
    const resp = await invoke_demo<CaptureResponse>("capture_fullscreen_demo", {
      clipboard: false,
      inlineBase64: true,
    });
    currentSelection = {
      rect: { x: 0, y: 0, width: resp.width, height: resp.height },
      width: resp.width,
      height: resp.height,
      base64Png: resp.base64Png,
      pinned: false,
    };
    message = `Captured fullscreen ${resp.width}×${resp.height}`;
  });
}

async function captureMonitor(id: number) {
  await run(async () => {
    message = `Capturing monitor ${id}…`;
    render();
    const resp = await invoke_demo<CaptureResponse>("capture_monitor_demo", {
      monitor: id,
      clipboard: false,
      inlineBase64: true,
    });
    currentSelection = {
      rect: { x: 0, y: 0, width: resp.width, height: resp.height },
      width: resp.width,
      height: resp.height,
      base64Png: resp.base64Png,
      pinned: false,
    };
    message = `Captured monitor ${id}: ${resp.width}×${resp.height}`;
  });
}

async function pinSelection() {
  if (!currentSelection?.base64Png) { message = "No selection to pin"; render(); return; }
  await run(async () => {
    const pin = await invoke_demo<{ id: number }>("pin_image", {
      options: { base64Image: currentSelection?.base64Png },
    });
    message = `Created pin ${pin.id}`;
    await refreshPins();
  });
}

async function annotateSelection() {
  if (!currentSelection?.base64Png) { message = "无截图可标注"; render(); return; }
  await run(async () => {
    await invoke_demo<void>("annotate_image_demo", { base64Image: currentSelection?.base64Png });
    message = "标注中：在悬浮工具栏选择工具，在编辑器窗口绘制";
  });
}

async function copySelection() {
  if (!currentSelection?.base64Png) { message = "No selection to copy"; render(); return; }
  await run(async () => {
    const pin = await invoke_demo<{ id: number }>("pin_image", {
      options: { base64Image: currentSelection?.base64Png },
    });
    await invoke_demo<void>("copy_pin", { id: pin.id });
    await invoke_demo<void>("close_pin", { id: pin.id });
    message = "Copied to clipboard";
    await refreshPins();
  });
}

// ─── Record tab actions ───────────────────────────────────────────────────────

async function selectRecordRegion() {
  await run(async () => {
    message = "框选录制区域，ESC 取消…";
    render();
    const result = await invoke_demo<[number, number, number, number] | null>("select_record_region");
    if (result) {
      recordRegion = result;
      recordTarget = "region";
      message = `已选定区域：${result[2]}×${result[3]}，起点 (${result[0]}, ${result[1]})`;
    } else {
      message = "区域选择已取消";
    }
  });
}

async function startRecording() {
  if (recordingId !== null) return;
  if (!recordOutput.trim()) { message = "请先填写输出路径"; render(); return; }
  if (recordTarget === "region" && !recordRegion) { message = "请先选定录制区域"; render(); return; }
  await run(async () => {
    const id = await invoke_demo<number>("start_recording_demo", {
      output: recordOutput.trim(),
      systemAudio,
      microphone: useMic,
      monitor: recordTarget === "monitor" ? (recordMonitor ?? undefined) : undefined,
      region: recordTarget === "region" ? recordRegion : undefined,
    });
    recordingId = id;
    recordingSeconds = 0;
    recordingTimer = setInterval(() => { recordingSeconds++; render(); }, 1000);
    if (recordTarget === "region" && recordRegion) {
      invoke_demo<void>("show_region_indicator", { rect: recordRegion }).catch(() => {});
    }
    message = `录屏已开始 (id ${id})`;
  });
}

async function stopRecording() {
  if (recordingId === null) return;
  const id = recordingId;
  await run(async () => {
    if (recordingTimer !== null) { clearInterval(recordingTimer); recordingTimer = null; }
    const path = await invoke_demo<string>("stop_recording_demo", { id });
    recordingId = null;
    message = `Saved: ${path}`;
  });
  invoke_demo<void>("hide_region_indicator").catch(() => {});
}

// ─── Pin tab actions ──────────────────────────────────────────────────────────

async function setPinOpacity(id: number, opacity: number) {
  await run(async () => {
    await invoke_demo<void>("set_pin_opacity", { options: { id, opacity } });
    message = `Pin ${id} opacity → ${Math.round(opacity * 100)}%`;
    await refreshPins();
  });
}

async function copyPin(id: number) {
  await run(async () => {
    await invoke_demo<void>("copy_pin", { id });
    message = `Pin ${id} copied to clipboard`;
  });
}

async function closePin(id: number) {
  await run(async () => {
    await invoke_demo<void>("close_pin", { id });
    message = `Pin ${id} closed`;
    await refreshPins();
  });
}

// ─── HTML fragments ───────────────────────────────────────────────────────────

function tabsHtml(): string {
  const tabs: { id: Tab; label: string }[] = [
    { id: "capture", label: "截图" },
    { id: "record", label: "录屏" },
    { id: "pins", label: `贴图${pins.length ? ` (${pins.length})` : ""}` },
  ];
  return `<nav class="tabs">${tabs
    .map(
      (t) =>
        `<button class="tab-btn${activeTab === t.id ? " active" : ""}" data-action="switch-tab" data-tab="${t.id}">${t.label}</button>`,
    )
    .join("")}</nav>`;
}

function captureTabHtml(): string {
  const previewHtml = currentSelection?.base64Png
    ? `<div class="preview-toolbar">
        <button data-action="annotate-selection" ${busy ? "disabled" : ""}>标注</button>
        <button data-action="pin-selection" ${busy ? "disabled" : ""}>贴到桌面</button>
        <button data-action="copy-selection" ${busy ? "disabled" : ""}>复制</button>
        <button data-action="clear-selection">清除</button>
       </div>
       <img class="preview-image" src="data:image/png;base64,${currentSelection.base64Png}" alt="capture" />
       <div class="meta">${currentSelection.width}×${currentSelection.height} · (${currentSelection.rect.x}, ${currentSelection.rect.y})</div>`
    : `<div class="empty">暂无截图</div>`;

  const monitorButtons =
    monitors.length > 0
      ? monitors
          .map((m) => {
            const label = m.primary ? `显示器 ${m.id}（主屏）` : `显示器 ${m.id}`;
            return `<div class="monitor-row">
              <div>
                <span class="monitor-label">${label}</span>
                <span class="meta">${m.rect.width}×${m.rect.height}</span>
              </div>
              <button data-action="capture-monitor" data-id="${m.id}" ${busy ? "disabled" : ""}>截图</button>
            </div>`;
          })
          .join("")
      : `<div class="meta" style="padding:8px 0">未检测到显示器</div>`;

  return `<div class="capture-grid">
    <section class="panel preview-panel">
      <div class="panel-heading">
        <h2>预览</h2>
        <button data-action="select-region" ${busy ? "disabled" : ""}>框选区域</button>
      </div>
      ${previewHtml}
    </section>
    <section class="panel direct-panel">
      <h2>直接截图</h2>
      <button class="full-btn" data-action="capture-fullscreen" ${busy ? "disabled" : ""}>全屏截图</button>
      <div class="monitor-list">${monitorButtons}</div>
    </section>
  </div>`;
}

function recordTabHtml(): string {
  const isRecording = recordingId !== null;
  const monitorOptions = monitors
    .map(
      (m) =>
        `<option value="${m.id}" ${recordMonitor === m.id ? "selected" : ""}>${m.primary ? `显示器 ${m.id}（主屏）` : `显示器 ${m.id}`} ${m.rect.width}×${m.rect.height}</option>`,
    )
    .join("");

  const regionLabel = recordRegion
    ? `${recordRegion[2]}×${recordRegion[3]}，起点 (${recordRegion[0]}, ${recordRegion[1]})`
    : "未选定";

  return `<div class="record-form">
    <label class="form-row">
      <span class="form-label">输出路径</span>
      <input class="form-input" type="text" id="record-output"
        placeholder="例：C:\\Users\\你的名字\\Desktop\\recording.mp4"
        value="${escapeHtml(recordOutput)}" ${isRecording ? "disabled" : ""} />
    </label>

    <div class="form-row">
      <span class="form-label">录制目标</span>
      <span class="radio-group">
        <label><input type="radio" name="rec-target" value="fullscreen" ${recordTarget === "fullscreen" ? "checked" : ""} ${isRecording ? "disabled" : ""} /> 全屏</label>
        <label><input type="radio" name="rec-target" value="monitor" ${recordTarget === "monitor" ? "checked" : ""} ${isRecording ? "disabled" : ""} /> 指定显示器</label>
        <select id="rec-monitor" ${recordTarget !== "monitor" || isRecording ? "disabled" : ""}>${monitorOptions || '<option value="">—</option>'}</select>
        <label><input type="radio" name="rec-target" value="region" ${recordTarget === "region" ? "checked" : ""} ${isRecording ? "disabled" : ""} /> 选定区域</label>
        <button data-action="select-record-region" ${isRecording || busy ? "disabled" : ""}>框选区域</button>
        ${recordTarget === "region" ? `<span class="region-hint">${escapeHtml(regionLabel)}</span>` : ""}
      </span>
    </div>

    <div class="form-row">
      <span class="form-label">音频</span>
      <span class="checkbox-group">
        <label><input type="checkbox" id="audio-system" ${systemAudio ? "checked" : ""} ${isRecording ? "disabled" : ""} /> 系统声音</label>
        <label><input type="checkbox" id="audio-mic" ${useMic ? "checked" : ""} ${isRecording ? "disabled" : ""} /> 麦克风</label>
      </span>
    </div>

    <div class="record-controls">
      <button class="record-btn${isRecording ? " recording" : ""}"
        data-action="${isRecording ? "stop-recording" : "start-recording"}"
        ${busy ? "disabled" : ""}>${isRecording ? "■ 停止录屏" : "▶ 开始录屏"}</button>
      ${isRecording ? `<span class="timer">${fmtTime(recordingSeconds)}</span>` : ""}
    </div>

    ${isRecording ? `<div class="record-status">正在录屏中…</div>` : ""}
  </div>`;
}

function pinsTabHtml(): string {
  if (pins.length === 0) return `<div class="empty compact">暂无贴图</div>`;
  return pins
    .map(
      (pin) => `<div class="pin-row">
      <div class="pin-info">
        <strong>#${pin.id}</strong>
        <span>${pin.displaySize.width}×${pin.displaySize.height}</span>
        <span>${pin.position.x}, ${pin.position.y}</span>
        <span>${Math.round(pin.opacity * 100)}%</span>
      </div>
      <div class="pin-actions">
        <button data-action="opacity-100" data-id="${pin.id}" ${busy ? "disabled" : ""}>100%</button>
        <button data-action="opacity-70" data-id="${pin.id}" ${busy ? "disabled" : ""}>70%</button>
        <button data-action="copy-pin" data-id="${pin.id}" ${busy ? "disabled" : ""}>复制</button>
        <button data-action="close-pin" data-id="${pin.id}" ${busy ? "disabled" : ""}>关闭</button>
      </div>
    </div>`,
    )
    .join("");
}

// ─── Render ───────────────────────────────────────────────────────────────────

function render() {
  let tabContent: string;
  if (activeTab === "capture") {
    tabContent = captureTabHtml();
  } else if (activeTab === "record") {
    tabContent = recordTabHtml();
  } else {
    tabContent = `<section class="panel pins-panel">
      <div class="panel-heading">
        <h2>贴图列表</h2>
        <button data-action="refresh-pins" ${busy ? "disabled" : ""}>刷新</button>
      </div>
      ${pinsTabHtml()}
    </section>`;
  }

  root.innerHTML = `<main>
    <header>
      <div><h1>win-screen Demo</h1><p>Windows 截图 · 录屏 · 桌面贴图</p></div>
      ${tabsHtml()}
    </header>
    <div class="tab-content">${tabContent}</div>
    <footer class="status">${escapeHtml(message)}</footer>
  </main>`;

  // bind live inputs
  const outputInput = document.getElementById("record-output") as HTMLInputElement | null;
  outputInput?.addEventListener("input", () => { recordOutput = outputInput.value; });

  const sysChk = document.getElementById("audio-system") as HTMLInputElement | null;
  sysChk?.addEventListener("change", () => { systemAudio = sysChk.checked; });

  const micChk = document.getElementById("audio-mic") as HTMLInputElement | null;
  micChk?.addEventListener("change", () => { useMic = micChk.checked; });

  document.querySelectorAll<HTMLInputElement>('input[name="rec-target"]').forEach((radio) => {
    radio.addEventListener("change", () => {
      recordTarget = radio.value as RecordTarget;
      if (recordTarget === "monitor" && monitors.length > 0 && recordMonitor === null) {
        recordMonitor = monitors[0].id;
      }
      render();
    });
  });

  const monSel = document.getElementById("rec-monitor") as HTMLSelectElement | null;
  monSel?.addEventListener("change", () => { recordMonitor = Number(monSel.value); });
}

// ─── Event delegation ─────────────────────────────────────────────────────────

root.addEventListener("click", (event) => {
  const target = event.target as HTMLElement;
  const button = target.closest<HTMLButtonElement>("button[data-action]");
  if (!button) return;

  const action = button.dataset.action;
  const id = Number(button.dataset.id);

  if (action === "switch-tab") {
    activeTab = (button.dataset.tab as Tab) ?? "capture";
    render();
    return;
  }
  if (busy) return;

  if (action === "select-region") void selectRegion();
  else if (action === "annotate-selection") void annotateSelection();
  else if (action === "capture-fullscreen") void captureFullscreen();
  else if (action === "capture-monitor") void captureMonitor(id);
  else if (action === "pin-selection") void pinSelection();
  else if (action === "copy-selection") void copySelection();
  else if (action === "clear-selection") { currentSelection = null; message = "Cleared"; render(); }
  else if (action === "refresh-pins") void loadPins();
  else if (action === "opacity-100") void setPinOpacity(id, 1);
  else if (action === "opacity-70") void setPinOpacity(id, 0.7);
  else if (action === "copy-pin") void copyPin(id);
  else if (action === "close-pin") void closePin(id);
  else if (action === "select-record-region") void selectRecordRegion();
  else if (action === "start-recording") void startRecording();
  else if (action === "stop-recording") void stopRecording();
});

// ─── Tauri event listeners ────────────────────────────────────────────────────

void listen<SelectionResponse>("win-screen-demo://selection-done", async (event) => {
  currentSelection = event.payload;
  const pinInfo =
    event.payload.pinned && event.payload.pinId ? `, pinned #${event.payload.pinId}` : "";
  message = `Selected ${event.payload.width}×${event.payload.height}${pinInfo}`;
  await refreshPins();
  render();
});

void listen("win-screen-demo://selection-canceled", () => {
  message = "Selection canceled";
  render();
});

void listen<string>("win-screen-demo://recording-stopped", (event) => {
  if (recordingTimer !== null) { clearInterval(recordingTimer); recordingTimer = null; }
  recordingId = null;
  invoke_demo<void>("hide_region_indicator").catch(() => {});
  message = `Recording saved: ${event.payload}`;
  render();
});

// ─── Init ─────────────────────────────────────────────────────────────────────

void Promise.all([loadPins(), loadMonitors()]).then(() => render());
render();
