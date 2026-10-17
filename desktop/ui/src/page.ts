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
  const fit = () => {
    const height = Math.ceil(content.getBoundingClientRect().height);
    if (height !== last) {
      last = height;
      void core.fitWindow(height);
    }
  };
  new ResizeObserver(fit).observe(content);
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
