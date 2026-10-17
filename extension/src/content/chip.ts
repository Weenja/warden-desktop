// The in-page points chip (standalone only, guarded sites the user allowed, off by default).
// It reads its numbers from storage.local ("chip", written by the background), reads nothing
// from the page and sends nothing. Tap it to shrink it to a dot; tap the dot to open it.
// It lives in a closed shadow root so the page's styles cannot change it (and it cannot change
// the page's).

import idleImg from "../../static/mochi/idle.webp";
import worriedImg from "../../static/mochi/worried.webp";
import type { ChipData } from "../background/chip.ts";
import { ext } from "../ext.ts";

const CSS = `
:host{all:initial}
.chip{position:fixed;right:16px;bottom:16px;z-index:2147483647;display:inline-flex;align-items:center;gap:8px;height:40px;padding:0 14px 0 6px;border-radius:20px;border:1.5px solid var(--f);background:var(--bg);color:var(--ink);font:800 15px/1 system-ui,-apple-system,"Segoe UI",sans-serif;font-variant-numeric:tabular-nums;box-shadow:0 8px 24px -8px rgba(0,0,0,.45);cursor:pointer}
.chip img{width:30px;height:30px}
.chip .rate{color:var(--tone)}
.chip.dot{width:22px;height:22px;padding:0;gap:0;border:3px solid var(--bg);background:var(--f)}
.chip.dot>*{display:none}
.mint{--f:#3DDC97;--tone:#0B7148}.amber{--f:#F5A623;--tone:#7E5300}
.chip{--bg:#fff;--ink:#1D1A1F}
@media (prefers-color-scheme:dark){.chip{--bg:#1C1920;--ink:#F4EFF6}.mint{--tone:#74E5B4}.amber{--tone:#FFCB6B}}
`;

const host = document.createElement("warden-chip");
const root = host.attachShadow({ mode: "closed" });
const style = document.createElement("style");
style.textContent = CSS;
const chip = document.createElement("button");
chip.className = "chip mint";
const img = document.createElement("img");
img.alt = "";
const pts = document.createElement("span");
const rate = document.createElement("span");
rate.className = "rate";
chip.append(img, pts, rate);
root.append(style, chip);
chip.addEventListener("click", () => chip.classList.toggle("dot"));

function render(data: ChipData | undefined): void {
  if (!data?.show) {
    host.remove();
    return;
  }
  chip.classList.toggle("mint", !data.low);
  chip.classList.toggle("amber", data.low);
  img.src = data.low ? worriedImg : idleImg;
  pts.textContent = `${data.points} pts`;
  rate.textContent = data.low ? `Low · −${data.perMin}/min` : `−${data.perMin}/min`;
  chip.setAttribute("aria-label", `${data.points} points, minus ${data.perMin} per minute. Tap to shrink.`);
  if (!host.isConnected) document.documentElement.append(host);
}

void ext.storage.local.get("chip").then((got) => render(got.chip as ChipData | undefined));
ext.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.chip) render(changes.chip.newValue as ChipData | undefined);
});
