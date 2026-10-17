// The first-run tab (opens once after install). Google sign-in also lands here, so it shows the
// sign-in result too.

import { GET_WARDEN_URL } from "../config.ts";
import { ext } from "../ext.ts";
import type { View } from "../view.ts";
import { $, command, followTheme, renderPermissions, show, tone, watchView } from "./common.ts";

followTheme();
renderPermissions($("perms"));
$<HTMLAnchorElement>("get-warden").href = GET_WARDEN_URL;

function status(toneName: "mint" | "grey", title: string, text: string): void {
  tone($("status-card"), toneName);
  $("status-title").textContent = title;
  $("status-text").textContent = text;
}

function render(v: View): void {
  const linked = v.mode === "linked";
  $("payload").textContent = linked
    ? '{ "type": "tab", "site": "youtube.com", "idle": false }'
    : '{ "site": "youtube.com", "seconds": 30 }';
  const needsSignIn = v.mode === "standalone" && !v.signedIn;
  show($("signin-card"), needsSignIn);
  show($("status-card"), !needsSignIn);
  $("hero-text").textContent = v.signedIn ? "Guarded sites now drain points here too." : "Sign in to start.";

  if (v.mode === "checking") status("grey", "Looking for the Warden app", "One moment.");
  else if (linked && v.signedIn) status("mint", `Linked to Warden on ${v.deviceName ?? "this computer"}`, `${v.points ?? 0} pts · same account as your phone`);
  else if (linked) status("grey", "Warden app found", "Open the Warden app and sign in. This extension links by itself.");
  else if (v.signedIn) status("mint", `Signed in as ${v.account ?? "you"}`, `${v.points ?? 0} pts · same account as your phone`);

  const s = v.signin;
  const noAccount = s.noAccountEmail !== null;
  $("signin-title").textContent = noAccount ? "No Warden account" : "Sign in";
  $("signin-text").textContent = noAccount
    ? `No Warden account for ${s.noAccountEmail}. Make one in the Warden app on your phone.`
    : "Same account as your phone.";
  $("google").lastChild!.textContent = noAccount ? "Use another account" : "Continue with Google";
  show($("get-warden"), noAccount);
  $("signin-error").textContent = s.error ?? "";
  $<HTMLButtonElement>("google").disabled = s.busy;
  $("google").dataset.other = String(noAccount);
}

$("google").addEventListener("click", async () => {
  // Same as the popup: ask for the Supabase origin first thing in the click (user gesture).
  const origins = ext.runtime.getManifest().host_permissions ?? [];
  if (!(await ext.permissions.request({ origins }))) return;
  await command({ cmd: "google", selectAccount: $("google").dataset.other === "true" });
});

watchView(render);
