// Sign-in helpers (no network here; see api.ts).
//
// Google sign-in uses OAuth with PKCE, like the desktop app:
// 1. We make a random `verifier` and put only its SHA-256 (`challenge`) in the authorize URL.
// 2. The user signs in with Google in a normal tab; we never see the password.
// 3. Supabase answers its own callback with a redirect to `warden://auth-callback?code=...`.
//    The background reads that redirect's Location header (see background/signin.ts), so the
//    link never reaches the OS (where the Warden phone app could catch it).
// 4. We trade `code` + `verifier` for a session. A code seen by anyone else is useless without
//    the verifier, which never leaves this extension.

import { REDIRECT_URI } from "./config.ts";

function base64url(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export async function challengeFor(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return base64url(new Uint8Array(digest));
}

/** 32 random bytes = 43 base64url characters, inside RFC 7636's 43..128 range. */
export function newVerifier(): string {
  return base64url(crypto.getRandomValues(new Uint8Array(32)));
}

/**
 * `GET /auth/v1/authorize` for Google. `selectAccount` adds `prompt=select_account`, which
 * Supabase passes on to Google so it shows its account chooser ("Use another account").
 */
export function authorizeUrl(supabaseUrl: string, challenge: string, selectAccount: boolean): string {
  const url = new URL(`${supabaseUrl}/auth/v1/authorize`);
  url.searchParams.set("provider", "google");
  url.searchParams.set("redirect_to", REDIRECT_URI);
  url.searchParams.set("code_challenge", challenge);
  url.searchParams.set("code_challenge_method", "s256");
  if (selectAccount) url.searchParams.set("prompt", "select_account");
  return url.toString();
}

export type CallbackResult = { code: string } | { error: string } | null;

/**
 * Read `warden://auth-callback?code=...`. Null when the URL is not our callback. Errors (the
 * user cancelled, Google refused) arrive in the query or the fragment.
 */
export function parseCallback(raw: string): CallbackResult {
  if (!raw.startsWith(REDIRECT_URI)) return null;
  const rest = raw.slice(REDIRECT_URI.length);
  if (rest !== "" && !"?#/".includes(rest[0])) return null;
  const [beforeHash, hash = ""] = rest.split("#", 2);
  const query = beforeHash.includes("?") ? beforeHash.slice(beforeHash.indexOf("?") + 1) : "";
  const params = new URLSearchParams(query);
  const fragment = new URLSearchParams(hash);
  const get = (key: string) => params.get(key) ?? fragment.get(key);
  const error = get("error_description") ?? get("error");
  if (error) return { error };
  const code = get("code");
  return code ? { code } : { error: "The sign-in link has no code." };
}

/** Light check before we ask the server to email a code. */
export function looksLikeEmail(s: string): boolean {
  const value = s.trim();
  const at = value.indexOf("@");
  if (at <= 0 || value.includes(" ")) return false;
  const domain = value.slice(at + 1);
  return domain.includes(".") && !domain.startsWith(".");
}
