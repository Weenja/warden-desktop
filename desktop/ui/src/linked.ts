// "This Mac is linked": the device name, the browsers found and the extension note.
import { core, type UiState } from "./core";
import { $, appTile, applyOsWording, followSystemTheme, fitWindowToContent, isWindows } from "./page";

followSystemTheme();
applyOsWording();
fitWindowToContent($("#content"));
if (isWindows) $("#device-icon").querySelector("use")?.setAttribute("href", "#i-win");

function render(state: UiState): void {
  $("#device-name").textContent = state.device?.name ?? "";
  const list = $("#browsers");
  list.replaceChildren(
    ...state.browsers.map((browser) => {
      const li = document.createElement("li");
      const name = document.createElement("span");
      name.className = "row__text row__title";
      name.textContent = browser.name;
      // The extension does not exist yet, so every browser drains while it is in front.
      const status = document.createElement("span");
      status.className = "status status--warn";
      status.textContent = "No extension";
      li.append(appTile(browser.name, browser.icon), name, status);
      return li;
    }),
  );
  $("#no-browsers").hidden = state.browsers.length > 0;
}

// "Open at login": shows the real login item state; a failed change flips the switch back.
const autostart = $<HTMLInputElement>("#autostart");
void core.getAutostart().then((enabled) => (autostart.checked = enabled));
autostart.addEventListener("change", async () => {
  try {
    autostart.checked = await core.setAutostart(autostart.checked);
  } catch {
    autostart.checked = !autostart.checked;
  }
});

$("#done").addEventListener("click", () => void core.closeSignin());
void core.onState(render);
void core.getState().then(render);
