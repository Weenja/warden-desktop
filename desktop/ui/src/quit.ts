// Quit confirm: quitting stops protection and tells the crew (a last heartbeat with stopped_at).
import { core, errorText, type UiState } from "./core";
import { $, crewAvatars, fitWindowToContent, followSystemTheme, nameList } from "./page";

followSystemTheme();
fitWindowToContent($("#content"));

function render(state: UiState): void {
  const crew = state.crew;
  $("#avatars").replaceChildren(...crewAvatars(crew));
  $("#avatars").hidden = crew.length === 0;
  $("#lead").textContent =
    crew.length === 0
      ? "Nothing is guarded on this computer until you open Warden again."
      : `${nameList(crew)} ${crew.length === 1 ? "gets" : "get"} told you stopped protection.`;
  $("#quit").textContent = crew.length === 0 ? "Quit Warden" : "Quit and tell my crew";
}

$("#keep").addEventListener("click", () => void core.closeWindow());
$<HTMLButtonElement>("#quit").addEventListener("click", async (event) => {
  const button = event.currentTarget as HTMLButtonElement;
  button.disabled = true;
  try {
    await core.quitWarden();
  } catch (e) {
    button.disabled = false;
    console.warn(errorText(e));
  }
});
void core.getState().then(render);
