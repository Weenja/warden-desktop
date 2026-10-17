import assert from "node:assert/strict";
import { test } from "node:test";
import * as d from "../src/drain.ts";

const NOW_S = 1_800_000_000;
const NOW_MS = NOW_S * 1000;

/** A balance token with the given claims (signature not checked, as in the extension). */
function token(balance: number, extra: Partial<d.BalanceClaims> = {}): string {
  const claims = { sub: "u", balance_seconds: balance, day_key: "2026-10-18", multiplier: 1, iat: NOW_S, exp: NOW_S + 900, ...extra };
  const b64 = Buffer.from(JSON.stringify(claims)).toString("base64url");
  return `eyJhbGciOiJIUzI1NiJ9.${b64}.sig`;
}

function anchored(balance: number, extra: Partial<d.BalanceClaims> = {}): d.DrainState {
  const s = d.newDrain(NOW_MS);
  d.anchor(s, token(balance, extra));
  return s;
}

test("balance token payload is read; junk is not", () => {
  assert.equal(d.decodeBalanceToken(token(2520))?.balance_seconds, 2520);
  assert.equal(d.decodeBalanceToken("not-a-jwt"), null);
  assert.equal(d.decodeBalanceToken("a.%%%.c"), null);
});

test("countdown: time on a guarded site comes off the local balance", () => {
  const s = anchored(600);
  d.advance(s, NOW_MS, "youtube.com");
  d.advance(s, NOW_MS + 45_000, "youtube.com");
  assert.equal(d.left(s), 555);
  assert.equal(d.points(s), 9);
  assert.equal(d.secondsToZero(s), 555);
});

test("bedtime multiplier drains twice as fast locally", () => {
  const s = anchored(600, { multiplier: 2 });
  d.advance(s, NOW_MS, "youtube.com");
  d.advance(s, NOW_MS + 60_000, "youtube.com");
  assert.equal(d.left(s), 480);
  assert.equal(d.perMinute(s), 2);
  assert.equal(d.secondsToZero(s), 240);
});

test("a long gap (sleep) is not counted", () => {
  const s = anchored(600);
  d.advance(s, NOW_MS, "youtube.com");
  d.advance(s, NOW_MS + (d.MAX_GAP_SECS + 1) * 1000, "youtube.com");
  assert.equal(d.left(s), 600);
});

test("ticks: 30 s for the site in front, any whole second after a change, evidence unique and reused on retry", () => {
  const s = anchored(600);
  d.advance(s, NOW_MS, "youtube.com");
  d.advance(s, NOW_MS + 20_000, "youtube.com");
  assert.equal(d.nextTick(s), null, "not due before 30 s");
  d.advance(s, NOW_MS + 35_000, "youtube.com");
  const first = d.nextTick(s);
  assert.deepEqual(first, { site: "youtube.com", seconds: 35, evidence: `x${NOW_MS}.0` });
  assert.deepEqual(d.nextTick(s), first, "retry reuses the same evidence");
  d.ticked(s, 565, false, "2026-10-18", token(565));
  d.advance(s, NOW_MS + 40_500, null); // switched away after 5.5 s
  const second = d.nextTick(s);
  assert.equal(second?.seconds, 5);
  assert.equal(second?.evidence, `x${NOW_MS}.1`);
});

test("locks at zero, at token expiry, and when the server shields with points left", () => {
  const zero = anchored(30);
  d.advance(zero, NOW_MS, "youtube.com");
  d.advance(zero, NOW_MS + 31_000, "youtube.com");
  assert.equal(d.lockReason(zero, NOW_S + 31), "zero");
  assert.equal(zero.current, null, "nothing drains once locked");
  assert.ok(d.nextTick(zero), "a tick is due at once at zero");

  const offline = anchored(600);
  assert.equal(d.lockReason(offline, NOW_S + 900), "offline");

  const max = anchored(600);
  d.advance(max, NOW_MS, "youtube.com");
  d.advance(max, NOW_MS + 30_000, "youtube.com");
  d.nextTick(max);
  d.ticked(max, 570, true, "2026-10-18", token(570));
  assert.equal(d.lockReason(max, NOW_S + 30), "daily_max");
  d.anchor(max, token(570, { day_key: "2026-10-19", iat: NOW_S + 1 }));
  assert.equal(d.lockReason(max, NOW_S + 30), null, "a new day lifts it");
});

test("a new token does not re-anchor while a tick is in flight", () => {
  const s = anchored(600);
  d.advance(s, NOW_MS, "youtube.com");
  d.advance(s, NOW_MS + 30_000, "youtube.com");
  d.nextTick(s);
  d.anchor(s, token(600, { iat: NOW_S + 5 }));
  assert.equal(d.left(s), 570, "in-flight seconds still count");
  d.dropInFlight(s);
  assert.equal(d.left(s), 600);
});
