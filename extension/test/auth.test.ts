import assert from "node:assert/strict";
import { test } from "node:test";
import { authorizeUrl, challengeFor, newVerifier, parseCallback } from "../src/auth.ts";

test("PKCE challenge matches the RFC 7636 example", async () => {
  assert.equal(await challengeFor("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
  assert.equal(newVerifier().length, 43);
});

test("authorize URL carries the redirect, the challenge and the chooser only when asked", () => {
  const url = new URL(authorizeUrl("https://x.supabase.co", "abc", false));
  assert.equal(url.pathname, "/auth/v1/authorize");
  assert.equal(url.searchParams.get("redirect_to"), "warden://auth-callback");
  assert.equal(url.searchParams.get("code_challenge_method"), "s256");
  assert.equal(url.searchParams.get("prompt"), null);
  assert.equal(new URL(authorizeUrl("https://x.supabase.co", "abc", true)).searchParams.get("prompt"), "select_account");
});

test("callback parsing: code, provider errors, and links that are not ours", () => {
  assert.deepEqual(parseCallback("warden://auth-callback?code=abc-123"), { code: "abc-123" });
  // What staging really sends when the user cancels (query and fragment).
  assert.deepEqual(
    parseCallback("warden://auth-callback?error=access_denied&error_description=cancelled+by+test#error=access_denied&sb="),
    { error: "cancelled by test" },
  );
  assert.deepEqual(parseCallback("warden://auth-callback?code="), { error: "The sign-in link has no code." });
  assert.equal(parseCallback("warden://auth-callbackX?code=abc"), null);
  assert.equal(parseCallback("https://evil.example/?code=abc"), null);
});
