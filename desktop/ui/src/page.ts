// Shared page helpers: OS theme, OS wording, element lookup, window sizing.
import { core } from "./core";

/** Follow the OS light/dark setting live. tokens.css is dark first; light sets data-theme. */
export function followSystemTheme(): void {
  const light = window.matchMedia("(prefers-color-scheme: light)");
  const apply = () => {
    if (light.matches) document.documentElement.dataset.theme = "light";
    else delete document.documentElement.dataset.theme;
  };
  apply();
  light.addEventListener("change", apply);
}

export const isWindows = navigator.userAgent.includes("Windows");
/** "Mac" or "PC", as the designs word it. */
export const computerWord = isWindows ? "PC" : "Mac";

export function $<T extends HTMLElement = HTMLElement>(selector: string, root: ParentNode = document): T {
  const el = root.querySelector<T>(selector);
  if (!el) throw new Error(`missing element ${selector}`);
  return el;
}

/** Fill every [data-computer] with "Mac" / "PC" and every [data-tray] with "menu bar" / "tray". */
export function applyOsWording(): void {
  document.querySelectorAll("[data-computer]").forEach((el) => (el.textContent = computerWord));
  document.querySelectorAll("[data-tray]").forEach((el) => (el.textContent = isWindows ? "tray" : "menu bar"));
}

/** Resize the window to the content after every layout change, so nothing scrolls or gapes. */
export function fitWindowToContent(content: HTMLElement): void {
  let last = 0;
  const fit = (force = false) => {
    const height = Math.ceil(content.getBoundingClientRect().height);
    if (force || height !== last) {
      last = height;
      void core.fitWindow(height, window.innerHeight);
    }
  };
  new ResizeObserver(() => fit()).observe(content);
  // The OS applies a new size a moment later. If the page still does not fit then (for
  // example the title bar took more room than measured), fit again with fresh numbers.
  addEventListener("resize", () => {
    if (document.documentElement.scrollHeight > window.innerHeight + 1) fit(true);
  });
  fit();
}

/** An SVG icon from the page's sprite. */
export function icon(name: string, extraClass = ""): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", `ico ${extraClass}`.trim());
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

/** The letter tile the designs use for an app without an icon. Colour is stable per app. */
export function letterTile(label: string, size: "" | "sm" | "xs" = ""): HTMLSpanElement {
  const tile = document.createElement("span");
  tile.className = size ? `app-icon app-icon--${size}` : "app-icon";
  let hash = 0;
  for (const ch of label) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  tile.style.setProperty("--c", `hsl(${hash % 360} 45% 38%)`);
  tile.textContent = (label.trim()[0] ?? "?").toUpperCase();
  tile.title = label;
  return tile;
}

/** A browser's real icon in a tile, or its letter. */
export function appTile(name: string, iconUrl: string | null, size: "" | "sm" = ""): HTMLSpanElement {
  if (!iconUrl) return letterTile(name, size);
  const tile = document.createElement("span");
  tile.className = (size ? `app-icon app-icon--${size}` : "app-icon") + " app-icon--img";
  const img = document.createElement("img");
  img.src = iconUrl;
  img.alt = "";
  tile.append(img);
  return tile;
}

/** Same low threshold as the phone and the drain pill: 10 points or less. */
export const LOW_POINTS = 10;

/** Whole points (1 point = 1 minute), never below 0. */
export function points(seconds: number | null | undefined): number {
  return Math.max(0, Math.floor((seconds ?? 0) / 60));
}

/** Battery fill in percent: the balance against the crew's daily max. */
export function batteryLevel(balanceSeconds: number | null, dailyMaxSeconds: number | null): number {
  const max = points(dailyMaxSeconds);
  return max > 0 ? Math.min(100, Math.round((points(balanceSeconds) / max) * 100)) : 0;
}

/** "mint" (no class), "low" or "zero", for the battery and the pill. */
export function tone(pts: number): "" | "low" | "zero" {
  return pts === 0 ? "zero" : pts <= LOW_POINTS ? "low" : "";
}

/** "−1/min", with a real minus sign as in the designs. */
export function perMinute(n: number): string {
  return `−${n}/min`;
}

/** Up to three crew initials in coloured circles. */
export function crewAvatars(names: string[]): HTMLSpanElement[] {
  const colours = ["#FFB4A2", "#9DF0CB", "#B5D8FF", "#FFE08A", "#E3C4FF"];
  return names.slice(0, 3).map((name, i) => {
    const a = document.createElement("span");
    a.textContent = (name[0] ?? "?").toUpperCase();
    a.style.setProperty("background", colours[i % colours.length] ?? "#B5D8FF");
    return a;
  });
}

/** "Emma", "Emma and Jake", "Emma, Jake and Olivia". */
export function nameList(names: string[]): string {
  return names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}
