// Standalone drain rules, free of the browser and the network so they can be tested. Same rules
// as the desktop app (desktop/src-tauri/src/drain.rs), for sites only:
//
// - A guarded site in the front tab drains while the browser is in use (not idle).
// - The balance shown is optimistic: the server's last balance minus what was used here and not
//   charged yet. Used seconds go out as spend ticks: every 30 s of use, at once when the site
//   changes, and at once at zero.
// - Each tick carries a one-time evidence key; a retry reuses it, so the server never charges a
//   tick twice.
// - Locked at local zero, when the balance token expired (we cannot trust the number any more),
//   or when the server says today's spending is used up.
// - The server stays authoritative: every answer re-anchors the local balance.
//
// The extension's background can be stopped by the browser at any time, so this state is plain
// JSON kept in storage, and time is measured from stored timestamps, never from running timers.

/** Send a tick once this much use is collected for the site in front. */
export const TICK_AFTER_SECS = 30;
/** The server takes at most 900 s per tick. */
export const MAX_TICK_SECS = 900;
/** Low battery, as on the phone: 10 points or less. */
export const LOW_POINTS = 10;
/**
 * A gap longer than this between two looks is not counted: the computer slept or the browser
 * was suspended. The background looks at least every 30 s (alarm) and on every tab or idle change.
 */
export const MAX_GAP_SECS = 90;

/** Claims of the server-signed balance token (HS256 JWT, 15 min TTL). The signature is the
 * server's business: every charge is re-checked there. We only read the numbers. */
export type BalanceClaims = {
  sub: string;
  balance_seconds: number;
  day_key: string;
  multiplier: number;
  iat: number;
  exp: number;
};

export function decodeBalanceToken(token: string): BalanceClaims | null {
  const payload = token.split(".")[1];
  if (!payload) return null;
  try {
    const json = atob(payload.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(payload.length / 4) * 4, "="));
    const claims = JSON.parse(json) as BalanceClaims;
    return typeof claims.balance_seconds === "number" && typeof claims.exp === "number" ? claims : null;
  } catch {
    return null;
  }
}

export type Tick = { site: string; seconds: number; evidence: string };
export type LockReason = "zero" | "offline" | "daily_max";

export type DrainState = {
  /** Makes evidence keys unique per drain state (the sequence makes them unique per tick). */
  evidencePrefix: string;
  nextSeq: number;
  /** The server's balance (seconds) from the last token or tick answer. Null = not known yet. */
  serverBalance: number | null;
  /** The balance token last anchored, to notice a new one. */
  anchoredToken: string | null;
  /** Bedtime makes the server charge double; the local estimate follows (ticks stay raw seconds). */
  multiplier: number;
  /** Unix seconds when the balance token expires. */
  expiresAt: number;
  /** Used, not yet sent, per guarded site. */
  unsent: Record<string, number>;
  inFlight: Tick | null;
  /** The guarded site draining now, and since when (ms). */
  current: string | null;
  since: number | null;
  /** The server shielded with points left: today's spending is used up until this day ends. */
  closedDay: string | null;
};

export function newDrain(nowMs: number): DrainState {
  return {
    evidencePrefix: `x${nowMs}`,
    nextSeq: 0,
    serverBalance: null,
    anchoredToken: null,
    multiplier: 1,
    expiresAt: 0,
    unsent: {},
    inFlight: null,
    current: null,
    since: null,
    closedDay: null,
  };
}

/** A new balance token (heartbeat or tick answer), or null when signed out. A tick in flight
 * blocks re-anchoring: its answer carries a balance that already includes it. */
export function anchor(d: DrainState, token: string | null): void {
  const claims = token ? decodeBalanceToken(token) : null;
  if (!token || !claims) {
    d.serverBalance = null;
    d.anchoredToken = null;
    return;
  }
  d.multiplier = Math.max(1, claims.multiplier);
  d.expiresAt = claims.exp;
  if (d.closedDay !== null && d.closedDay !== claims.day_key) d.closedDay = null;
  if (d.inFlight === null && d.anchoredToken !== token) {
    d.serverBalance = claims.balance_seconds;
    d.anchoredToken = token;
  }
}

/** Seconds guarded sites can still use: the server's balance minus everything used here and not
 * charged yet (unsent and in flight), times the multiplier. */
export function left(d: DrainState): number {
  if (d.serverBalance === null) return 0;
  const unsent = Object.values(d.unsent).reduce((a, b) => a + b, 0);
  const inFlight = d.inFlight?.seconds ?? 0;
  return d.serverBalance - (unsent + inFlight) * d.multiplier;
}

/** Whole points left (1 point = 1 minute). */
export function points(d: DrainState): number {
  return Math.floor(Math.max(0, left(d)) / 60);
}

export function perMinute(d: DrainState): number {
  return Math.max(1, Math.round(d.multiplier));
}

export function lockReason(d: DrainState, nowSec: number): LockReason | null {
  if (left(d) <= 0) return "zero";
  if (nowSec >= d.expiresAt) return "offline";
  if (d.closedDay !== null) return "daily_max";
  return null;
}

/**
 * Count the time since the last look for the site that was draining, then decide what drains
 * from now on. `front` = the guarded site in the front tab while the browser is in use, else null.
 */
export function advance(d: DrainState, nowMs: number, front: string | null): void {
  if (d.current !== null && d.since !== null) {
    const gap = (nowMs - d.since) / 1000;
    if (gap > 0 && gap <= MAX_GAP_SECS) d.unsent[d.current] = (d.unsent[d.current] ?? 0) + gap;
  }
  const canDrain = d.serverBalance !== null && lockReason(d, Math.floor(nowMs / 1000)) === null;
  d.current = canDrain ? front : null;
  d.since = d.current !== null ? nowMs : null;
}

/**
 * The tick to send now, if any. While one is in flight it is returned again with the same
 * evidence (a retry). Due: 30 s collected for the site in front, any whole second for a site no
 * longer in front, or any whole second at zero.
 */
export function nextTick(d: DrainState): Tick | null {
  if (d.inFlight) return d.inFlight;
  const atZero = left(d) <= 0;
  // Fractions of a second for sites no longer in front are dropped, as on Android.
  for (const [site, secs] of Object.entries(d.unsent)) {
    if (site !== d.current && secs < 1) delete d.unsent[site];
  }
  const due = Object.entries(d.unsent).find(
    ([site, secs]) => secs >= TICK_AFTER_SECS || (secs >= 1 && (site !== d.current || atZero)),
  );
  if (!due) return null;
  const [site, collected] = due;
  const seconds = Math.min(Math.floor(collected), MAX_TICK_SECS);
  d.unsent[site] = collected - seconds;
  const tick = { site, seconds, evidence: `${d.evidencePrefix}.${d.nextSeq}` };
  d.nextSeq += 1;
  d.inFlight = tick;
  return tick;
}

/** The server charged the tick in flight. */
export function ticked(d: DrainState, balance: number, shield: boolean, dayKey: string, token: string): void {
  d.inFlight = null;
  anchor(d, token); // the answer's token: new expiry and multiplier
  d.serverBalance = balance;
  // Points left but the server shields: today's spending is used up.
  if (shield && balance > 0) d.closedDay = dayKey;
}

/** The server refused the tick in flight for good: forget those seconds. (A network failure
 * keeps it in flight, so the next try reuses its evidence.) */
export function dropInFlight(d: DrainState): void {
  d.inFlight = null;
}

/** Wall-clock seconds until the site draining now reaches zero, or null when nothing drains. */
export function secondsToZero(d: DrainState): number | null {
  if (d.current === null) return null;
  return Math.max(0, left(d)) / d.multiplier;
}
