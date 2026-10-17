// The page a guarded site is replaced with at 0 points (blocked.html?site=youtube.com).

import { $, command, followTheme } from "./common.ts";

followTheme();

const site = new URLSearchParams(location.search).get("site") ?? "this site";
$("site").textContent = site;
$("text").textContent = `Out of points. Earn more to use ${site}.`;
if (matchMedia("(max-width: 600px)").matches) document.querySelector(".blocked")?.classList.add("blocked--compact");

// History here is [page before, the guarded site (blocked), this page]: two steps back leave the
// guarded site behind. With nothing before it, "back" closes the tab.
$("back").addEventListener("click", () => {
  if (history.length > 2) history.go(-2);
  else void command({ cmd: "close_me" });
});
$("close").addEventListener("click", () => void command({ cmd: "close_me" }));
