// Shared by the extension's pages: light/dark from the OS, the view, and commands.

import { ext } from "../ext.ts";
import { readView, type Command, type View } from "../view.ts";

/** tokens.css is dark first; light needs `data-theme="light"`. Follow the OS, live. */
export function followTheme(): void {
  const light = matchMedia("(prefers-color-scheme: light)");
  const apply = () => (document.documentElement.dataset.theme = light.matches ? "light" : "dark");
  apply();
  light.addEventListener("change", apply);
}

/** Call `render` now and whenever the background publishes a new view. */
export function watchView(render: (view: View) => void): void {
  void readView().then(render);
  ext.storage.onChanged.addListener((changes, area) => {
    if (area === "session" && changes.view?.newValue) render(changes.view.newValue as View);
  });
}

export function command(c: Command): Promise<unknown> {
  return ext.runtime.sendMessage(c);
}

export const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

export function show(el: HTMLElement, visible: boolean): void {
  el.hidden = !visible;
}

/** Set the tone class (mint, amber, coral, grey) on a component. */
export function tone(el: HTMLElement, name: "mint" | "amber" | "coral" | "grey"): void {
  el.classList.remove("tone-mint", "tone-amber", "tone-coral", "tone-grey");
  el.classList.add(`tone-${name}`);
}

/** The permissions panel lists exactly what the manifest asks for. */
export function permissionList(): { name: string; why: string }[] {
  const why: Record<string, string> = {
    tabs: "See which site is in the front tab",
    declarativeNetRequest: "Show the blocked page at 0 pts",
    nativeMessaging: "Talk to the Warden app on this computer",
    idle: "Stop draining when you step away",
    alarms: "Count the minutes",
    storage: "Remember your guarded sites and sign-in",
    webRequest: "Finish Google sign-in without leaving the browser",
    webRequestBlocking: "Keep the sign-in link inside the browser",
    scripting: "Show the points chip on guarded sites, if you turn it on",
  };
  const manifest = ext.runtime.getManifest();
  return (manifest.permissions ?? []).map((name) => ({ name, why: why[name] ?? "" }));
}

export function renderPermissions(list: HTMLElement): void {
  list.replaceChildren(
    ...permissionList().map(({ name, why }) => {
      const li = document.createElement("li");
      const code = document.createElement("code");
      code.textContent = name;
      li.append(code, why);
      return li;
    }),
  );
  const hosts = ext.runtime.getManifest().host_permissions ?? [];
  for (const host of hosts) {
    const li = document.createElement("li");
    const code = document.createElement("code");
    code.textContent = new URL(host.replace("*", "x")).host;
    li.append(code, "The Warden server (standalone sign-in and points)");
    list.append(li);
  }
}
