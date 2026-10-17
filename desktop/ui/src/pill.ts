// The drain pill at the notch (Mac) / top centre (Windows). Collapsed: icon and points.
// Hover or click opens it: what drains, the rate, the battery and Stop.
import { core, type DrainView, type UiState } from "./core";
import { $, appTile, batteryLevel, icon, perMinute, points, tone } from "./page";

const pill = $("#pill");
/** The middle column on screens without a notch: the pill is one short bar. */
const NO_NOTCH_GAP = 8;

let hovered = false;
let pinned = false;
let shownFor: string | null = null;

/** Line the middle column up with the notch and the height with the menu bar. */
async function fitToScreen(): Promise<void> {
  const m = await core.pillMetrics();
  pill.style.setProperty("--notch-w", `${m.notch_width > 0 ? m.notch_width : NO_NOTCH_GAP}px`);
  pill.style.setProperty("--bar-h", `${m.bar_height}px`);
}

function moa(face: string): HTMLImageElement {
  const img = document.createElement("img");
  img.className = "moa";
  img.alt = "";
  img.src = `/mochi/${face}.webp`;
  return img;
}

/** Collapsed pill: a lock at zero, else the app's icon, or a tiny MOA when it has none. */
function leadIcon(drain: DrainView): Node {
  if (drain.locked) return icon("lock");
  if (!drain.icon) return moa("idle");
  const tile = appTile(drain.label, drain.icon, "sm");
  tile.classList.replace("app-icon--sm", "app-icon--xs");
  return tile;
}

/** Open pill title: locked MOA at zero, else the app's icon (or MOA). */
function titleIcon(drain: DrainView): Node {
  if (drain.locked) return moa("locked");
  return drain.icon ? appTile(drain.label, drain.icon, "sm") : moa("idle");
}

function render(state: UiState): void {
  const drain = state.drain;
  if (!drain) {
    shownFor = null;
    pinned = false;
    return;
  }
  if (shownFor !== drain.app_id) {
    // A new app in front: start closed, and re-measure (it may be on another screen).
    shownFor = drain.app_id;
    pinned = false;
    void fitToScreen();
  }
  const pts = points(drain.balance_seconds);
  const t = drain.locked ? "zero" : tone(pts) || (drain.kind === "browser" ? "noext" : "");
  pill.className = ["pill", t && `pill--${t}`, hovered || pinned ? "is-open" : ""].filter(Boolean).join(" ");

  $("#lead").replaceChildren(leadIcon(drain));
  $("#pts").textContent = `${pts} pts`;

  const rate = perMinute(drain.per_minute);
  const title = $("#title");
  const sub = $("#sub");
  const hint = $("#hint");
  if (drain.locked) {
    title.replaceChildren(titleIcon(drain), `${drain.label} · 0 pts`);
    sub.textContent = "MOA locks it now.";
  } else if (drain.kind === "browser") {
    title.replaceChildren(titleIcon(drain), `${drain.label} · drains while it's open`);
    sub.textContent = drain.low
      ? `Low battery · no Warden extension · ${rate}`
      : `No Warden extension in ${drain.label} · ${rate}`;
  } else {
    title.replaceChildren(titleIcon(drain), `${drain.label} · ${pts} pts left`);
    sub.textContent = drain.low ? `Low battery · ${rate} · MOA locks it at 0` : `${rate} while it's open`;
  }
  hint.textContent = drain.kind === "browser"
    ? `Stop hides this. ${drain.label} keeps draining while it's in front.`
    : drain.kind === "site"
      ? `Stop closes the ${drain.label} tab.`
      : `Stop closes ${drain.label}.`;
  $("#actions").hidden = drain.locked;
  hint.hidden = drain.locked;

  const level = batteryLevel(drain.balance_seconds, state.daily_max_seconds);
  $("#battery").className = drain.locked ? "battery battery--zero" : drain.low ? "battery battery--low" : "battery";
  $("#battery-fill").style.setProperty("--level", `${level}%`);
}

let last: UiState | null = null;
function update(state: UiState): void {
  last = state;
  render(state);
}
function rerender(): void {
  if (last) render(last);
}

// The window is exactly as big as the pill, so nothing around it blocks the menu bar.
new ResizeObserver(() => {
  const r = pill.getBoundingClientRect();
  void core.pillFit(Math.ceil(r.width), Math.ceil(r.height));
}).observe(pill);

// Hover: the OS panel reports it (web views see no hover while Warden is not the active app);
// the DOM events cover the rest.
void core.onPillHover((inside) => {
  hovered = inside;
  rerender();
});
pill.addEventListener("mouseenter", () => {
  hovered = true;
  rerender();
});
pill.addEventListener("mouseleave", () => {
  hovered = false;
  rerender();
});
pill.addEventListener("click", (event) => {
  if ((event.target as Element).closest("button")) return;
  pinned = !pinned;
  rerender();
});
$("#stop").addEventListener("click", () => void core.stopDraining());

void fitToScreen();
void core.onState(update);
void core.getState().then(update);
