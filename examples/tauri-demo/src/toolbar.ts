import { invoke } from "@tauri-apps/api/core";
import "./toolbar.css";

document.addEventListener("click", (event) => {
  const target = event.target as HTMLElement;
  const button = target.closest<HTMLButtonElement>("button[data-action]");
  if (!button) return;

  void invoke("toolbar_decide", {
    options: { action: button.dataset.action ?? "confirm" },
  });
});
