// The toolbar popup. Renders the view the background publishes; buttons send commands.

import { GET_WARDEN_URL } from "../config.ts";
import { ext } from "../ext.ts";
import { sitePatterns } from "../background/chip.ts";
import type { View } from "../view.ts";
import { $, command, followTheme, renderPermissions, show, tone, watchView } from "./common.ts";

followTheme();
renderPermissions($("perms"));
$<HTMLAnchorElement>("get-warden").href = GET_WARDEN_URL;

let current: View | null = null;
let emailFormOpen = false;

function setMochi(img: string, name: string): void {
  $<HTMLImageElement>(img).src = `mochi/${name}.webp`;
}

function renderBalance(v: View): void {
  const points = v.points ?? 0;
  $("points").firstChild!.textContent = String(points);
  const battery = $("battery");
  show(battery, v.dailyMax !== null && v.dailyMax > 0);
  if (v.dailyMax) battery.style.setProperty("--v", String(Math.min(points / v.dailyMax, 1)));
}

function renderDrainRow(v: View): void {
  const row = $("drain");
  const icon = $("site-ico");
  const title = $("drain-title");
  const sub = $("drain-sub");
  row.classList.remove("tone-grey");
  icon.replaceChildren();
  if (v.locked) {
    icon.innerHTML = '<svg class="ico"><use href="#i-lock"/></svg>';
    title.textContent = "Guarded sites are locked";
    sub.textContent = "Earn on your phone: steps, focus, tasks";
  } else if (v.guardedSite && v.draining) {
    icon.textContent = v.guardedSite[0].toUpperCase();
    title.textContent = `${v.guardedSite} · −${v.perMin}/min`;
    sub.textContent = v.low ? "Low battery · MOA locks it at 0" : `${v.points ?? 0} pts left · guarded`;
  } else if (v.guardedSite) {
    icon.textContent = v.guardedSite[0].toUpperCase();
    title.textContent = `${v.guardedSite} · paused`;
    sub.textContent = "Guarded. No drain while you are away.";
  } else {
    row.classList.add("tone-grey");
    icon.textContent = v.site ? v.site[0].toUpperCase() : "·";
    title.textContent = v.site ? `${v.site} · free` : "No site in this tab";
    sub.textContent = v.site ? "Not guarded. No drain." : "Nothing to count here.";
  }
}

function renderSites(v: View): void {
  show($("sites"), v.guardedSites.length > 0);
  $("sites-list").replaceChildren(
    ...v.guardedSites.map((site) => {
      const li = document.createElement("li");
      li.textContent = site;
      if (site === v.guardedSite) li.classList.add("is-here");
      return li;
    }),
  );
}

function renderSignedIn(v: View): void {
  const main = $("main");
  const draining = !v.locked && v.guardedSite !== null && v.draining;
  tone(main, v.locked ? "coral" : v.low ? "amber" : "mint");
  $("tag").textContent = v.locked ? "Locked" : v.low ? "Low battery" : draining ? "Draining" : v.mode === "linked" ? "Linked" : "Signed in";
  setMochi("mochi", v.locked ? "locked" : v.low ? "worried" : draining ? "guard" : "happy");
  renderBalance(v);
  renderDrainRow(v);
  show($("balance"), true);
  show($("drain"), true);
  show($("stop"), draining);
  show($("close-tab"), v.locked && v.closable);
  renderSites(v);
  show($("note"), !draining && !v.locked && !v.guardedSite);
  show($("chip-setting"), v.mode === "standalone" && v.guardedSites.length > 0);
  $("chip-switch").setAttribute("aria-checked", String(v.chip));
}

function renderMessage(mochi: string, title: string, text: string): void {
  setMochi("msg-mochi", mochi);
  $("msg-title").textContent = title;
  $("msg-text").textContent = text;
  show($("msg"), true);
}

function renderSignIn(v: View): void {
  const s = v.signin;
  tone($("main"), "grey");
  $("tag").textContent = "Not signed in";
  show($("signin"), true);
  const codeStep = s.codeSentTo !== null;
  const noAccount = s.noAccountEmail !== null;
  show($("signin-start"), !codeStep && !noAccount && !emailFormOpen);
  show($("email-form"), !codeStep && !noAccount && emailFormOpen);
  show($("code-form"), codeStep);
  show($("no-account"), noAccount);
  $("code-note").textContent = `We emailed a 6-digit code to ${s.codeSentTo}.`;
  $("signin-error").textContent = s.error ?? "";
  for (const b of $("signin").querySelectorAll("button")) (b as HTMLButtonElement).disabled = s.busy;
  if (noAccount) {
    renderMessage("sideeye", "No Warden account", `No Warden account for ${s.noAccountEmail}. Make one in the Warden app on your phone.`);
  } else if (s.busy && !codeStep) {
    renderMessage("wink", "Finish in the new tab", "Sign in with Google there, then come back.");
  } else {
    renderMessage("wink", "Sign in to Warden", "Use the same account as your phone.");
  }
}

function renderFooter(v: View): void {
  let icon = "#i-eye";
  let text = "Looking for the Warden app on this computer";
  if (v.mode === "linked") {
    icon = "#i-laptop";
    const where = v.deviceName ?? "this computer";
    text = v.signedIn ? `Linked to Warden on ${where}` : `Warden app found on ${where}`;
  } else if (v.mode === "standalone") {
    icon = v.signedIn ? "#i-check" : "#i-eye";
    text = v.signedIn ? `Signed in as ${v.account ?? "you"}` : "No Warden app here. Works on its own.";
  }
  $("foot-ico").setAttribute("href", icon);
  $("foot-text").textContent = text;
}

function renderFacts(v: View): void {
  const toApp = v.mode === "linked";
  $("fact-site").textContent = toApp
    ? "The site in the front tab, domain only, to the Warden app on this computer"
    : "The guarded site in the front tab, domain only";
  $("fact-time").textContent = toApp ? "Whether you are away (no input for 5 min)" : "How many seconds it was open";
  $("payload").textContent = toApp
    ? '{ "type": "tab", "site": "youtube.com", "idle": false }'
    : '{ "site": "youtube.com", "seconds": 30 }';
  show($("sign-out"), v.mode === "standalone" && v.signedIn);
}

function render(v: View): void {
  current = v;
  for (const id of ["balance", "drain", "stop", "close-tab", "note", "msg", "open-app", "signin", "chip-setting", "sites"]) {
    show($(id), false);
  }
  if (v.mode === "checking") {
    tone($("main"), "grey");
    $("tag").textContent = "Looking";
    renderMessage("idle", "Looking for Warden", "One moment.");
  } else if (v.signedIn) {
    renderSignedIn(v);
  } else if (v.mode === "linked") {
    tone($("main"), "grey");
    $("tag").textContent = "Signed out";
    renderMessage("sleepy", "Sign in on this computer", "Open the Warden app and sign in. This extension links by itself.");
    show($("open-app"), true);
  } else {
    renderSignIn(v);
  }
  renderFooter(v);
  renderFacts(v);
}

// ----- Buttons -----

$("stop").addEventListener("click", () => void command({ cmd: "stop" }));
$("close-tab").addEventListener("click", () => void command({ cmd: "stop" }));
$("open-app").addEventListener("click", () => void command({ cmd: "show_app" }));
$("sign-out").addEventListener("click", () => {
  void command({ cmd: "sign_out" });
  show($("facts"), false);
  show($("main"), true);
});
$("facts-open").addEventListener("click", () => {
  show($("main"), false);
  show($("facts"), true);
});
$("facts-close").addEventListener("click", () => {
  show($("facts"), false);
  show($("main"), true);
});

/** Google sign-in reads the Supabase redirect, which needs access to the Supabase origin. Ask
 * first thing in the click: Firefox allows a permission prompt only directly in a user input
 * handler (no await before it). Already granted = no prompt. */
async function googleSignIn(selectAccount: boolean): Promise<void> {
  const origins = ext.runtime.getManifest().host_permissions ?? [];
  if (!(await ext.permissions.request({ origins }))) return;
  await command({ cmd: "google", selectAccount });
}

$("google").addEventListener("click", () => void googleSignIn(false));
$("google-other").addEventListener("click", () => void googleSignIn(true));
$("email-start").addEventListener("click", () => {
  emailFormOpen = true;
  if (current) render(current);
  $("email").focus();
});
$("email-cancel").addEventListener("click", () => {
  emailFormOpen = false;
  if (current) render(current);
});
$("email-form").addEventListener("submit", (e) => {
  e.preventDefault();
  void command({ cmd: "email_send", email: $<HTMLInputElement>("email").value });
});
$("code-form").addEventListener("submit", (e) => {
  e.preventDefault();
  void command({ cmd: "email_verify", code: $<HTMLInputElement>("code").value });
});
$("code-back").addEventListener("click", () => void command({ cmd: "signin_reset" }));

$("chip-switch").addEventListener("click", async () => {
  const on = !current?.chip;
  // The chip needs access to the guarded sites; ask on this click (user gesture).
  if (on && !(await ext.permissions.request({ origins: sitePatterns(current?.guardedSites ?? []) }))) return;
  await command({ cmd: "chip", on });
});

watchView(render);
