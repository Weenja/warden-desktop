// The standalone sign-in session, refreshed before it expires.
//
// Where it lives: `storage.local`, inside this browser profile. An extension has no access to
// the OS keychain, so this is the safest place it has: only this extension can read it, and it
// is never written to logs or sent anywhere but the Warden API.
//
// Refresh tokens rotate: each refresh returns a new one and the old one dies. Two parallel
// refreshes would race and one would fail, so refreshing is single-flight.

import { ApiError, refreshSession, type TokenResponse } from "../api.ts";
import { ext } from "../ext.ts";

/** Refresh this long before the access token expires, so a request never leaves with a token
 * that dies in flight. */
const REFRESH_MARGIN_SECS = 120;
const KEY = "session";

export type Session = {
  access_token: string;
  refresh_token: string;
  /** Unix seconds when `access_token` expires. */
  expires_at: number;
  user_id: string;
  email: string | null;
};

export class SignedOut extends Error {}

const nowSecs = () => Math.floor(Date.now() / 1000);

export function sessionFrom(t: TokenResponse): Session {
  return {
    access_token: t.access_token,
    refresh_token: t.refresh_token,
    expires_at: t.expires_at ?? nowSecs() + t.expires_in,
    user_id: t.user.id,
    email: t.user.email ?? null,
  };
}

export async function loadSession(): Promise<Session | null> {
  const got = await ext.storage.local.get(KEY);
  return (got[KEY] as Session | undefined) ?? null;
}

export const saveSession = (s: Session) => ext.storage.local.set({ [KEY]: s });
export const clearSession = () => ext.storage.local.remove(KEY);

let refreshing: Promise<string> | null = null;

/** Refresh once for every caller waiting at the same time. */
function refreshOnce(old: Session): Promise<string> {
  refreshing ??= (async () => {
    try {
      const fresh = sessionFrom(await refreshSession(old.refresh_token));
      fresh.email ??= old.email;
      // Save before use: the old refresh token is already dead on the server.
      await saveSession(fresh);
      return fresh.access_token;
    } catch (e) {
      // 400/401: the refresh token was revoked, reused or expired. The user signs in again.
      // Network errors keep the session for the next try.
      if (e instanceof ApiError && (e.status === 400 || e.status === 401)) {
        await clearSession();
        throw new SignedOut();
      }
      throw e;
    } finally {
      refreshing = null;
    }
  })();
  return refreshing;
}

/** A valid access token, refreshed first when it is about to expire. */
export async function accessToken(): Promise<string> {
  if (refreshing) return refreshing;
  const s = await loadSession();
  if (!s) throw new SignedOut();
  return s.expires_at - nowSecs() <= REFRESH_MARGIN_SECS ? refreshOnce(s) : s.access_token;
}

/** Run `call` with a valid token. On 401 (revoked token, clock skew) refresh once and retry,
 * unless another caller already refreshed while this one was waiting. */
export async function authed<T>(call: (token: string) => Promise<T>): Promise<T> {
  const token = await accessToken();
  try {
    return await call(token);
  } catch (e) {
    if (!(e instanceof ApiError) || e.status !== 401) throw e;
    const s = await loadSession();
    if (!s) throw new SignedOut();
    const fresh = s.access_token !== token ? s.access_token : await refreshOnce(s);
    return call(fresh);
  }
}
