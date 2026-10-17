// The panel under the menu bar / tray icon: balance, battery, what is guarded, who is told.
import { core, errorText, type UiState } from "./core";
import { $, appTile, applyOsWording, followSystemTheme, fitWindowToContent, letterTile } from "./page";

followSystemTheme();
applyOsWording();
fitWindowToContent($("#content"));

/** Same low threshold as the phone and the drain pill: 10 points or less. */
const LOW_POINTS = 10;

function points(seconds: number | null): number {
  return Math.max(0, Math.floor((seconds ?? 0) / 60));
}

function renderBalance(state: UiState): void {
  const pts = points(state.balance_seconds);
  const max = points(state.daily_max_seconds);
  const level = max > 0 ? Math.min(100, Math.round((pts / max) * 100)) : 0;
  const tone = pts === 0 ? "zero" : pts <= LOW_POINTS ? "low" : "";
  const face = pts === 0 ? "locked" : tone === "low" ? "worried" : "idle";
  $<HTMLImageElement>("#face").src = `/mochi/${face}.webp`;
  $("#balance").textContent = String(pts);
  $("#mood").textContent = max > 0 ? `Nothing draining · battery ${level}%` : "Nothing draining";
  $("#battery").className = tone ? `battery battery--${tone}` : "battery";
  $("#battery-fill").style.setProperty("--level", `${level}%`);
  $("#offline").hidden = state.online;
}

function renderGuarded(state: UiState): void {
  $("#guarded-apps").replaceChildren(...state.guarded_apps.map((app) => letterTile(app.label)));
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
      status.className = "status status--warn";
      status.textContent = "No extension";
      row.append(appTile(browser.name, browser.icon, "sm"), name, status);
      return row;
    }),
  );
}

/** "Emma, Jake and Olivia see it if you stop Warden." */
function renderCrew(state: UiState): void {
  const names = state.crew;
  $("#crew").hidden = names.length === 0;
  const avatarColours = ["#FFB4A2", "#9DF0CB", "#B5D8FF", "#FFE08A", "#E3C4FF"];
  $("#avatars").replaceChildren(
    ...names.slice(0, 3).map((name, i) => {
      const a = document.createElement("span");
      a.textContent = (name[0] ?? "?").toUpperCase();
      a.style.setProperty("background", avatarColours[i % avatarColours.length] ?? "#B5D8FF");
      return a;
    }),
  );
  const who = names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  $("#crew-text").textContent = `${who} ${names.length === 1 ? "sees" : "see"} it if you stop Warden.`;
}

function render(state: UiState): void {
  $("#signed-in").hidden = !state.signed_in;
  $("#signed-out").hidden = state.signed_in;
  if (!state.signed_in) return;
  renderBalance(state);
  renderGuarded(state);
  renderBrowsers(state);
  renderCrew(state);
  $("#email").textContent = state.email ?? "";
}

$("#sign-in").addEventListener("click", () => void core.openSignin());
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

void core.onState(render);
void core.getState().then(render);
