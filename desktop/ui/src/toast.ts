// The one-time low battery note, top right. Closes itself after a few seconds (Rust) or on click.
import { core, type UiState } from "./core";
import { $, fitWindowToContent, followSystemTheme, perMinute, points } from "./page";

followSystemTheme();
fitWindowToContent($("#content"));

function render(state: UiState): void {
  const drain = state.drain;
  if (!drain) return;
  $("#title").textContent = `Low battery · ${points(drain.balance_seconds)} pts`;
  $("#text").textContent = `${drain.label} drains ${perMinute(drain.per_minute)}. MOA locks it at 0.`;
}

$("#content").addEventListener("click", () => void core.closeWindow());
void core.getState().then(render);
