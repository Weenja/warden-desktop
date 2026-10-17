import assert from "node:assert/strict";
import { test } from "node:test";
import { JUST_CREATED_SECS, SAME_MOMENT_SECS, unixFromRfc3339, verdict, type AuthUser } from "../src/new-account.ts";

const NOW = 1_792_238_400; // 2026-10-17T12:00:00Z
const iso = (unix: number) => new Date(unix * 1000).toISOString();
const googleMade = (age: number): AuthUser => ({
  id: "u",
  created_at: iso(NOW - age),
  identities: [{ provider: "google", created_at: iso(NOW - age) }],
});

test("young account a Google sign-in made, with no device, is rejected", () => {
  assert.equal(verdict(googleMade(0), NOW, []), "reject");
  assert.equal(verdict(googleMade(JUST_CREATED_SECS - 1), NOW, []), "reject");
  const later = { ...googleMade(60), identities: [{ provider: "google", created_at: iso(NOW - 60 + SAME_MOMENT_SECS) }] };
  assert.equal(verdict(later, NOW, []), "reject");
});

test("old account, any device, or clock skew keeps it", () => {
  assert.equal(verdict(googleMade(JUST_CREATED_SECS), NOW, []), "keep");
  assert.equal(verdict(googleMade(60), NOW, ["android"]), "keep");
  assert.equal(verdict(googleMade(60), NOW, ["windows"]), "keep");
  assert.equal(verdict(googleMade(-1), NOW, []), "keep", "created in the future");
  assert.equal(verdict({ ...googleMade(60), created_at: null }, NOW, []), "keep");
});

test("Google linked later, or not exactly one Google identity, keeps it", () => {
  assert.equal(verdict({ id: "u", created_at: iso(NOW - 180), identities: [{ provider: "google", created_at: iso(NOW - 60) }] }, NOW, []), "keep");
  const linked = { id: "u", created_at: iso(NOW - 60), identities: [{ provider: "google", created_at: iso(NOW - 60 + SAME_MOMENT_SECS + 1) }] };
  assert.equal(verdict(linked, NOW, []), "keep");
  const two = { ...googleMade(60), identities: [{ provider: "google", created_at: iso(NOW - 60) }, { provider: "email", created_at: iso(NOW - 60) }] };
  assert.equal(verdict(two, NOW, []), "keep");
  assert.equal(verdict({ ...googleMade(60), identities: [] }, NOW, []), "keep", "anonymous");
  assert.equal(verdict({ ...googleMade(60), identities: null }, NOW, []), "keep");
  assert.equal(verdict({ ...googleMade(60), identities: [{ provider: "email", created_at: iso(NOW - 60) }] }, NOW, []), "keep");
  assert.equal(verdict({ ...googleMade(60), identities: [{ provider: "google", created_at: null }] }, NOW, []), "keep");
});

test("GoTrue timestamps", () => {
  assert.equal(unixFromRfc3339("2026-10-17T12:00:00Z"), NOW);
  assert.equal(unixFromRfc3339("2026-10-17T12:00:00.123456Z"), NOW);
  assert.equal(unixFromRfc3339("2026-10-17T17:30:00+05:30"), NOW);
  assert.equal(unixFromRfc3339("2026-10-17T11:00:00.5-01:00"), NOW);
  for (const bad of ["", "2026-10-17", "2026-10-17T12:00:00", "2026-13-17T12:00:00Z", "2026-10-17T12:00:00.Z", "soon"]) {
    assert.equal(unixFromRfc3339(bad), null, bad);
  }
});
