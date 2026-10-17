// Standalone mode (no Warden desktop app: Firefox on Android, or a computer without the app):
// the extension talks to the Warden API itself, with the same endpoints as the desktop app.
//
// Each `step` is one look at the world: sync with the server when due, count the time since the
// last look, send due spend ticks, and set the alarm for the moment points reach zero. All state
// is in storage, because the browser stops the background whenever it likes.

import * as api from "../api.ts";
import { ApiError } from "../api.ts";
import * as drain from "../drain.ts";
import { ext } from "../ext.ts";
import { guardedSiteFor } from "../site.ts";
import type { Front } from "./front.ts";
import { authed, clearSession, loadSession, SignedOut } from "./session.ts";

/** The crew sees a device that stays silent for hours as "went dark"; once a minute is plenty
 * and far below the server's rate limit. The block list is read on the same beat. */
const SYNC_EVERY_MS = 60_000;
/** At most this many ticks per step; more wait for the next step. */
const MAX_TICKS_PER_STEP = 5;
export const ZERO_ALARM = "zero";

type Solo = {
  deviceId: string | null;
  sites: string[];
  dailyMaxSeconds: number | null;
  drain: drain.DrainState;
  syncedAt: number;
};

export type SoloView = {
  signedIn: boolean;
  account: string | null;
  points: number | null;
  dailyMax: number | null;
  low: boolean;
  locked: boolean;
  perMin: number;
  guardedSite: string | null;
  draining: boolean;
  guardedSites: string[];
};

const freshSolo = (): Solo => ({
  deviceId: null,
  sites: [],
  dailyMaxSeconds: null,
  drain: drain.newDrain(Date.now()),
  syncedAt: 0,
});

async function loadSolo(): Promise<Solo> {
  const got = await ext.storage.local.get("solo");
  return (got.solo as Solo | undefined) ?? freshSolo();
}

const saveSolo = (s: Solo) => ext.storage.local.set({ solo: s });

/** A random id for this browser install. Not secret: it lets `register_device` find the same
 * device row again after a restart or a new sign-in. */
async function installId(): Promise<string> {
  const got = await ext.storage.local.get("installId");
  if (typeof got.installId === "string") return got.installId;
  const id = crypto.randomUUID();
  await ext.storage.local.set({ installId: id });
  return id;
}

/** `runtime.getPlatformInfo().os` to the server's platform names. ChromeOS and the BSDs are
 * reported as linux, the closest the server knows. */
function platformOf(os: string): api.Platform {
  if (os === "mac") return "macos";
  if (os === "win") return "windows";
  if (os === "android") return "android";
  return "linux";
}

const OS_NAMES: Record<api.Platform, string> = { macos: "Mac", windows: "Windows", linux: "Linux", android: "Android" };

/** "Firefox on Android", "Edge on Windows": the name the crew sees for this device. */
function browserName(): string {
  const brands = (navigator as Navigator & { userAgentData?: { brands: { brand: string }[] } }).userAgentData?.brands;
  const names = brands?.map((b) => b.brand) ?? [];
  if (navigator.userAgent.includes("Firefox/")) return "Firefox";
  if (names.includes("Microsoft Edge")) return "Edge";
  if (names.includes("Brave") || "brave" in navigator) return "Brave";
  if (names.includes("Opera")) return "Opera";
  return "Chrome";
}

async function register(solo: Solo): Promise<void> {
  const platform = platformOf((await ext.runtime.getPlatformInfo()).os);
  const name = `${browserName()} on ${OS_NAMES[platform]}`;
  const id = await installId();
  const version = ext.runtime.getManifest().version;
  const device = await authed((t) => api.registerDevice(t, platform, name, id, version));
  solo.deviceId = device.id;
}

async function sync(solo: Solo, deviceId: string): Promise<void> {
  const version = ext.runtime.getManifest().version;
  const beat = await authed((t) => api.heartbeat(t, deviceId, version));
  drain.anchor(solo.drain, beat.balance_token);
  solo.dailyMaxSeconds = beat.wallet.daily_max_seconds ?? null;
  const targets = await authed((t) => api.blockTargets(t));
  solo.sites = targets.filter((t) => t.kind === "site").map((t) => t.token.toLowerCase());
  solo.syncedAt = Date.now();
}

/** Send due ticks. A network failure keeps the tick in flight for the next step (same evidence);
 * a refusal (4xx) drops it so the local estimate stays honest. */
async function sendTicks(solo: Solo, deviceId: string): Promise<void> {
  for (let i = 0; i < MAX_TICKS_PER_STEP; i++) {
    const tick = drain.nextTick(solo.drain);
    if (!tick) return;
    await saveSolo(solo); // the evidence key must survive a stop before the answer
    try {
      const res = await authed((t) => api.spendTick(t, deviceId, tick.site, tick.seconds, tick.evidence));
      drain.ticked(solo.drain, res.balance_seconds, res.shield, res.wallet.day_key, res.balance_token);
    } catch (e) {
      const refused = e instanceof ApiError && e.status >= 400 && e.status < 500 && e.status !== 429;
      if (!refused) throw e;
      drain.dropInFlight(solo.drain);
      // 404 "site is not guarded": the list changed on the phone. Read it on the next step.
      if (e.status === 404) solo.syncedAt = 0;
    }
  }
}

/** The guarded site that drains now: in the front tab, browser in use. */
function drainingSite(front: Front, sites: readonly string[]): string | null {
  return front.focused && !front.idle ? guardedSiteFor(front.site, sites) : null;
}

export async function step(front: Front): Promise<SoloView> {
  const session = await loadSession();
  if (!session) return signedOutView();
  const solo = await loadSolo();
  const now = Date.now();
  try {
    if (!solo.deviceId) await register(solo);
    const deviceId = solo.deviceId as string;
    if (now - solo.syncedAt >= SYNC_EVERY_MS) await sync(solo, deviceId);
    drain.advance(solo.drain, now, drainingSite(front, solo.sites));
    await sendTicks(solo, deviceId);
  } catch (e) {
    if (e instanceof SignedOut) {
      await signOut();
      return signedOutView();
    }
    // Offline or a server error: keep counting locally; the next step retries.
    console.warn("Warden sync failed:", e instanceof Error ? e.message : e);
  }
  // Look again with the answers in: at zero this stops the drain right away.
  drain.advance(solo.drain, Date.now(), drainingSite(front, solo.sites));
  await saveSolo(solo);
  await setZeroAlarm(solo.drain);
  return soloView(solo, session.email);
}

/** Wake the background the moment points run out, so the site locks on time. */
async function setZeroAlarm(d: drain.DrainState): Promise<void> {
  const secs = drain.secondsToZero(d);
  if (secs === null) await ext.alarms.clear(ZERO_ALARM);
  else await ext.alarms.create(ZERO_ALARM, { when: Date.now() + secs * 1000 + 500 });
}

function soloView(solo: Solo, email: string | null): SoloView {
  const d = solo.drain;
  const known = d.serverBalance !== null;
  const points = known ? drain.points(d) : null;
  return {
    signedIn: true,
    account: email,
    points,
    dailyMax: solo.dailyMaxSeconds ? Math.floor(solo.dailyMaxSeconds / 60) : null,
    low: points !== null && points <= drain.LOW_POINTS,
    locked: known && drain.lockReason(d, Math.floor(Date.now() / 1000)) !== null,
    perMin: drain.perMinute(d),
    guardedSite: d.current,
    draining: d.current !== null,
    guardedSites: solo.sites,
  };
}

function signedOutView(): SoloView {
  return {
    signedIn: false,
    account: null,
    points: null,
    dailyMax: null,
    low: false,
    locked: false,
    perMin: 1,
    guardedSite: null,
    draining: false,
    guardedSites: [],
  };
}

/** A new session was saved: start from a clean slate for this account. */
export async function startFresh(): Promise<void> {
  await saveSolo(freshSolo());
}

export async function signOut(): Promise<void> {
  await clearSession();
  await saveSolo(freshSolo());
  await ext.alarms.clear(ZERO_ALARM);
}
