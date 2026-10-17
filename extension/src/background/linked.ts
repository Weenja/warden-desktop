// Linked mode: the Warden desktop app on this computer does the counting, the network calls,
// the pill and the countdown. The extension only tells it which site is in front (and whether
// the user is away) over native messaging, and acts on its commands. No network calls here.
//
// The browser starts the app's native host when we connect; "host not found" means the app is
// not installed, and the extension runs standalone.

import { NATIVE_HOST } from "../config.ts";
import { ext } from "../ext.ts";
import type { AppState, FromApp, ToApp } from "../protocol.ts";

/** The host may need to start the app first; give it this long to answer `hello`. */
const HELLO_TIMEOUT_MS = 10_000;

export type LinkedEvents = {
  onState: (state: AppState) => void;
  onCloseTab: (site: string) => void;
  /** The connection ended after it was up (app quit, host crashed, update). */
  onLost: () => void;
};

let port: chrome.runtime.Port | null = null;
let lastTab: string | null = null;

/** Connect and say hello. True when the app answered; false when there is no app. */
export function connect(events: LinkedEvents): Promise<boolean> {
  disconnect();
  if (typeof ext.runtime.connectNative !== "function") return Promise.resolve(false); // Firefox on Android
  return new Promise((resolve) => {
    let linked = false;
    let p: chrome.runtime.Port;
    try {
      p = ext.runtime.connectNative(NATIVE_HOST);
    } catch {
      resolve(false);
      return;
    }
    port = p;
    const timer = setTimeout(() => {
      if (!linked) {
        disconnect();
        resolve(false);
      }
    }, HELLO_TIMEOUT_MS);

    p.onMessage.addListener((raw: unknown) => {
      const msg = raw as FromApp;
      if (msg.type === "hello" && !linked) {
        linked = true;
        clearTimeout(timer);
        resolve(true);
      } else if (msg.type === "state" && linked) {
        events.onState(msg);
      } else if (msg.type === "close_tab" && linked) {
        events.onCloseTab(msg.site);
      }
    });
    p.onDisconnect.addListener(() => {
      // Reading lastError marks it handled ("Specified native messaging host not found." etc).
      void ext.runtime.lastError;
      if (port === p) port = null;
      clearTimeout(timer);
      if (linked) events.onLost();
      else resolve(false);
    });
    lastTab = null;
    send({ type: "hello", ext_version: ext.runtime.getManifest().version });
  });
}

export function disconnect(): void {
  const p = port;
  port = null;
  p?.disconnect();
}

export const isConnected = () => port !== null;

export function send(msg: ToApp): void {
  port?.postMessage(msg);
}

/** Report the front tab, only when it changed (the app reacts to every message). */
export function reportTab(site: string | null, idle: boolean): void {
  const key = JSON.stringify([site, idle]);
  if (key === lastTab) return;
  lastTab = key;
  send({ type: "tab", site, idle });
}
