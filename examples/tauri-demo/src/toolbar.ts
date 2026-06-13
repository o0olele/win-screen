import { invoke } from "@tauri-apps/api/core";
import "./toolbar.css";

// Send one action string to the active annotation surface (the selection overlay,
// or the standalone editor). core does the drawing; we only report button presses.
function send(action: string) {
  void invoke("annotation_command", { options: { action } });
}

function setActive(nodes: NodeListOf<Element>, target: Element | null) {
  nodes.forEach((n) => n.classList.toggle("active", n === target));
}

const tools = document.querySelectorAll<HTMLButtonElement>("button.tool");
const swatches = document.querySelectorAll<HTMLButtonElement>("button.swatch");

document.addEventListener("click", (event) => {
  const el = (event.target as HTMLElement).closest<HTMLElement>(
    "[data-tool],[data-action],[data-color]",
  );
  if (!el) return;

  if (el.dataset.tool) {
    // Clicking the already-active tool deselects it → back to selection mode.
    if (el.classList.contains("active")) {
      setActive(tools, null);
      send("tool:none");
    } else {
      setActive(tools, el);
      send(`tool:${el.dataset.tool}`);
    }
  } else if (el.dataset.color) {
    setActive(swatches, el);
    send(`color:${el.dataset.color}`);
  } else if (el.dataset.action) {
    send(el.dataset.action);
  }
});

const colorPick = document.getElementById("color-pick") as HTMLInputElement | null;
colorPick?.addEventListener("input", () => {
  setActive(swatches, null);
  send(`color:${colorPick.value}`);
});

const width = document.getElementById("width") as HTMLInputElement | null;
width?.addEventListener("input", () => {
  send(`width:${width.value}`);
});

// ESC cancels the whole capture/annotation.
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") send("cancel");
});
