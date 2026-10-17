// The panel under the menu bar / tray icon: balance, battery, what is guarded, who is told.
import { core, type UiState } from "./core";
import {
  $,
  appTile,
  applyOsWording,
  batteryLevel,
  computerWord,
  crewAvatars,
  followSystemTheme,
  fitWindowToContent,
  icon,
  nameList,
  perMinute,
  points,
  tone,
} from "./page";

followSystemTheme();
applyOsWording();
fitWindowToContent($("#content"));

function renderBalance(state: UiState): void {
  const pts = points(state.balance_seconds);
  const hasMax = points(state.daily_max_seconds) > 0;
  const level = batteryLevel(state.balance_seconds, state.daily_max_seconds);
  const t = tone(pts);
  // MOA's face follows what drains: locked at 0, worried when low, side-eye while draining.
  const face = pts === 0 ? "locked" : t === "low" ? "worried" : state.drain ? "sideeye" : "idle";
  $<HTMLImageElement>("#face").src = `/mochi/${face}.webp`;
  $("#balance").textContent = String(pts);
  const battery = hasMax ? `Battery ${level}%` : "";
  $("#mood").textContent = state.drain ? battery : ["Nothing draining", battery.toLowerCase()].filter(Boolean).join(" · ");
  $("#battery").className = t ? `battery battery--${t}` : "battery";
  $("#battery-fill").style.setProperty("--level", `${level}%`);
  $("#offline").hidden = state.online;
}

/** "Steam on this Mac  −1/min" while something drains, else "Nothing draining". */
function renderNow(state: UiState): void {
  const now = $("#now");
  const drain = state.drain;
  if (!drain) {
    now.className = "popover__now popover__now--idle";
    now.replaceChildren("Nothing draining");
    return;
  }
  now.className = drain.locked ? "popover__now popover__now--locked" : "popover__now";
  const text = document.createElement("span");
  text.textContent = drain.locked ? `${drain.label} is locked` : `${drain.label} on this ${computerWord}`;
  const rate = document.createElement("b");
  rate.textContent = drain.locked ? "0 pts" : perMinute(drain.per_minute);
  const lead = drain.locked ? icon("lock") : appTile(drain.label, drain.icon, "sm");
  now.replaceChildren(lead, text, rate);
}

function renderGuarded(state: UiState): void {
  $("#guarded-apps").replaceChildren(...state.guarded_apps.map((app) => appTile(app.label, app.icon)));
  $("#no-apps").hidden = state.guarded_apps.length > 0;
  const n = state.guarded_sites;
  $("#sites").textContent = n === 0 ? "No sites guarded" : `Sites: ${n} from your phone`;
}

function renderBrowsers(state: UiState): void {
  $("#browsers").hidden = state.browsers.length === 0;
  $("#browsers").replaceChildren(
    ...state.browsers.map((browser) => {
      const row = document.createElement("div");
      row.className = "row";
      const name = document.createElement("span");
      name.className = "row__text";
      name.textContent = browser.name;
      const status = document.createElement("span");
      status.className = browser.extension ? "status status--ok" : "status status--warn";
      if (browser.extension) status.append(icon("check"));
      status.append(browser.extension ? "Linked" : "No extension");
      row.append(appTile(browser.name, browser.icon, "sm"), name, status);
      return row;
    }),
  );
}

/** "Emma, Jake and Olivia see it if you stop Warden." */
function renderCrew(state: UiState): void {
  const names = state.crew;
  $("#crew").hidden = names.length === 0;
  $("#avatars").replaceChildren(...crewAvatars(names));
  $("#crew-text").textContent = `${nameList(names)} ${names.length === 1 ? "sees" : "see"} it if you stop Warden.`;
}

function render(state: UiState): void {
  $("#signed-in").hidden = !state.signed_in;
  $("#signed-out").hidden = state.signed_in;
  if (!state.signed_in) return;
  renderBalance(state);
  renderNow(state);
  renderGuarded(state);
  renderBrowsers(state);
  renderCrew(state);
  $("#email").textContent = state.email ?? "";
}

$("#sign-in").addEventListener("click", () => void core.openSignin());
$("#edit").addEventListener("click", () => void core.openPicker());
// Quitting tells the crew, so it asks first (the quit window).
$("#quit").addEventListener("click", () => void core.openQuit());

void core.onState(render);
void core.getState().then(render);
