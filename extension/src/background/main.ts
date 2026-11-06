// The extension's background (a service worker in Chrome, an event page in Firefox).
//
// It picks the mode, watches the front tab, and keeps the view, the badge and the blocking in
// step. Linked (Warden desktop app found): report the front tab to the app and follow its state.
// Standalone: count and charge through the Warden API (standalone.ts).
//
// The browser may stop this script whenever it is idle, so every listener is registered at the
// top level (the browser wakes us for them) and all state is kept in storage.

import { BROWSER } from "../target.ts";
import { ext } from "../ext.ts";
import type { AppState } from "../protocol.ts";
import { guardedSiteFor, siteOf } from "../site.ts";
import { EMPTY_VIEW, type Command, type Mode, type View } from "../view.ts";
import { paintBadge } from "./badge.ts";
import { blockOpenTabs, blockTabIfGuarded, isBlockedPage, setBlockRule } from "./blocker.ts";
import { publishChip, syncChipScript } from "./chip.ts";
import { IDLE_AFTER_SECS, readFront, type Front } from "./front.ts";
import * as linked from "./linked.ts";
import * as signin from "./signin.ts";
import * as standalone from "./standalone.ts";

/** While standalone, look for a newly installed desktop app this often. */
const NATIVE_RECHECK_MS = 5 * 60_000;
/** The heartbeat of the background: count, tick, sync. Chrome's shortest alarm is 30 s. */
const TICK_ALARM = "tick";

let mode: Mode = "checking";
let appState: AppState | null = null;

// ----- One refresh at a time -----

let chain: Promise<unknown> = Promise.resolve();
function serial<T>(fn: () => Promise<T>): Promise<T> {
  const run = chain.then(fn, fn);
  chain = run.catch(() => undefined);
  return run;
}

// ----- Mode -----

const linkedEvents: linked.LinkedEvents = {
  onState(state) {
    appState = state;
    void refresh();
  },
  onCloseTab(site) {
    void closeFrontTabIfOn(site);
  },
  onLost() {
    // The app quit or restarted. Look again shortly; standalone if it is gone for good.
    appState = null;
    mode = "checking";
    void refresh();
    setTimeout(() => void pickMode(true).then(refresh), 2_000);
  },
};

/** Linked when the desktop app's native host answers `hello`, else standalone. A recent
 * "standalone" verdict is reused (so a waking background does not start the host each time). */
async function pickMode(force: boolean): Promise<void> {
  const saved = await ext.storage.session.get(["mode", "nativeCheckedAt"]);
  const recent = Date.now() - ((saved.nativeCheckedAt as number | undefined) ?? 0) < NATIVE_RECHECK_MS;
  if (!force && saved.mode === "standalone" && recent) {
    mode = "standalone";
    return;
  }
  const isLinked = await linked.connect(linkedEvents);
  mode = isLinked ? "linked" : "standalone";
  await ext.storage.session.set({ mode, nativeCheckedAt: Date.now() });
}

// ----- Refresh: view, badge, blocking -----

function linkedPart(): Partial<View> {
  const s = appState;
  if (!s) return { signedIn: false };
  return {
    signedIn: s.signed_in,
    deviceName: s.device_name,
    points: s.signed_in ? s.points : null,
    dailyMax: s.daily_max ?? null,
    low: s.low,
    locked: s.signed_in && s.locked,
    perMin: s.draining?.per_min ?? 1,
    draining: s.draining !== null,
    guardedSites: s.guarded_sites.map((x) => x.toLowerCase()),
  };
}

async function buildView(front: Front): Promise<View> {
  let part: Partial<View> = {};
  if (mode === "linked") {
    linked.reportTab(front.site, front.idle);
    part = linkedPart();
  } else if (mode === "standalone") {
    part = await standalone.step(front);
  }
  const chipOn = (await ext.storage.local.get("chipOn")).chipOn === true;
  const view: View = { ...EMPTY_VIEW, ...part, mode, site: front.site, chip: chipOn, signin: await signin.readSignin() };
  view.guardedSite = guardedSiteFor(front.site, view.guardedSites);
  view.closable = view.guardedSite !== null || isBlockedPage(front.url);
  return view;
}

/** Block rule on while locked; at the moment of locking, open guarded tabs get the blocked page. */
async function applyLock(view: View): Promise<void> {
  const locked = view.locked && view.guardedSites.length > 0;
  const was = (await ext.storage.session.get("wasLocked")).wasLocked === true;
  await setBlockRule(locked ? view.guardedSites : []);
  if (locked && !was) await blockOpenTabs(view.guardedSites);
  if (locked !== was) await ext.storage.session.set({ wasLocked: locked });
}

async function applyChip(view: View): Promise<void> {
  const solo = view.mode === "standalone" && view.signedIn;
  await syncChipScript(view.chip && solo, view.guardedSites);
  await publishChip({
    show: solo && view.chip && !view.locked && view.points !== null,
    points: view.points ?? 0,
    perMin: view.perMin,
    low: view.low,
  });
}

const refresh = () =>
  serial(async () => {
    const view = await buildView(await readFront());
    await ext.storage.session.set({ view });
    await paintBadge(view);
    await applyLock(view);
    await applyChip(view);
  });

const ready = (async () => {
  // Create only when missing: re-creating restarts its countdown, and we wake often.
  if (!(await ext.alarms.get(TICK_ALARM))) await ext.alarms.create(TICK_ALARM, { periodInMinutes: 0.5 });
  await pickMode(false);
  await refresh();
})();

// ----- Tabs -----

async function closeFrontTabIfOn(site: string): Promise<void> {
  const front = await readFront();
  if (front.tabId !== null && guardedSiteFor(front.site, [site])) await ext.tabs.remove(front.tabId);
}

/** A tab navigated: while locked, a guarded site gets the blocked page at once. */
async function onTabUrl(tabId: number, url: string): Promise<void> {
  const { view } = await ext.storage.session.get("view");
  const v = view as View | undefined;
  if (v?.locked && guardedSiteFor(siteOf(url), v.guardedSites)) await blockTabIfGuarded(tabId, url, v.guardedSites);
}

const whenReady = (fn: () => unknown) => () => void ready.then(fn);

ext.tabs.onActivated.addListener(whenReady(refresh));
ext.tabs.onRemoved.addListener(whenReady(refresh));
ext.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.url) void onTabUrl(tabId, change.url);
  if (tab.active && (change.url !== undefined || change.audible !== undefined)) void ready.then(refresh);
});
ext.windows?.onFocusChanged.addListener(whenReady(refresh));
if (ext.idle) {
  ext.idle.setDetectionInterval(IDLE_AFTER_SECS);
  ext.idle.onStateChanged.addListener(whenReady(refresh));
}

ext.alarms.onAlarm.addListener((alarm) => {
  void ready.then(async () => {
    if (alarm.name === TICK_ALARM && mode === "standalone") {
      const { nativeCheckedAt } = await ext.storage.session.get("nativeCheckedAt");
      if (Date.now() - ((nativeCheckedAt as number | undefined) ?? 0) >= NATIVE_RECHECK_MS) await pickMode(true);
    }
    await refresh();
  });
});

// The user allowed sites for the points chip (popup switch): register it there.
ext.permissions.onAdded.addListener(whenReady(refresh));

// ----- First run -----

ext.runtime.onInstalled.addListener((details) => {
  if (details.reason === "install") void ext.tabs.create({ url: ext.runtime.getURL("welcome.html") });
});

// ----- Sign-in (standalone) -----

ext.webRequest.onHeadersReceived.addListener(
  signin.onCallbackHeaders,
  { urls: [signin.CALLBACK_URL_PATTERN], types: ["main_frame" as chrome.webRequest.ResourceType] },
  // Firefox can cancel the redirect to warden:// (blocking); Chrome MV3 can only observe.
  BROWSER === "firefox" ? ["blocking", "responseHeaders"] : ["responseHeaders"],
);

signin.onSignedIn(async () => {
  await serial(standalone.startFresh);
  await refresh();
});

// ----- Commands from the popup, first-run and blocked pages -----

async function handle(command: Command, sender: chrome.runtime.MessageSender): Promise<void> {
  switch (command.cmd) {
    case "google":
      return signin.startGoogle(command.selectAccount);
    case "email_send":
      return signin.sendCode(command.email);
    case "email_verify":
      return signin.verifyCode(command.code);
    case "signin_reset":
      return signin.resetSignin();
    case "sign_out":
      await serial(standalone.signOut);
      return signin.resetSignin();
    case "stop": {
      // Never close a tab that is not guarded: the popup button must not act as a general "close tab".
      const front = await readFront();
      const { view } = await ext.storage.session.get("view");
      const guarded = (view as View | undefined)?.guardedSites ?? [];
      const closable = guardedSiteFor(front.site, guarded) !== null || isBlockedPage(front.url);
      if (front.tabId !== null && closable) await ext.tabs.remove(front.tabId);
      return;
    }
    case "close_me":
      if (sender.tab?.id !== undefined) await ext.tabs.remove(sender.tab.id);
      return;
    case "show_app":
      return linked.send({ type: "show_app" });
    case "chip":
      await ext.storage.local.set({ chipOn: command.on });
      return;
  }
}

ext.runtime.onMessage.addListener((message, sender, reply) => {
  void ready
    .then(() => handle(message as Command, sender))
    .then(refresh)
    .finally(() => reply(true));
  return true; // answer asynchronously
});
