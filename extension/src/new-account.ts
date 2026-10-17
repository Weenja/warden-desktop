// Accounts are made on the phone only. Same rule as the desktop app
// (desktop/src-tauri/src/new_account.rs); keep the two in step.
//
// Email sign-in asks the server not to create accounts (`create_user: false`). Google sign-in
// cannot: when the Google account is unknown, Supabase makes a new, empty Warden account during
// the sign-in. Left alone, that account would link this browser with 0 points and no phone, and
// it would own the Google identity, so the user's phone could later fail to sign in with it.
// So right after a Google sign-in, before anything is saved, we look for that case and undo it
// with the user's own `delete-account` call.
//
// SAFETY INVARIANT: delete ONLY when every one of these holds, else keep:
// - the account is younger than JUST_CREATED_SECS (a `created_at` in the future = keep),
// - it has no device at all (phone or computer),
// - it has exactly one identity, and that identity is Google (this sign-in),
// - that identity was made within SAME_MOMENT_SECS of the account, so this sign-in CREATED the
//   account. A phone account starts anonymous and links Google later, so its Google identity is
//   clearly newer than the account.
// Any missing or unreadable field keeps the account. Only `verdict` decides, and it is tested.

export const JUST_CREATED_SECS = 10 * 60;
export const SAME_MOMENT_SECS = 5;

export type AuthUser = {
  id: string;
  email?: string | null;
  created_at?: string | null;
  identities?: { provider?: string | null; created_at?: string | null }[] | null;
};

export type Verdict = "keep" | "reject";

/** GoTrue timestamps (`2026-10-17T12:00:00.123456Z` or with `+hh:mm`) as Unix seconds, else null. */
export function unixFromRfc3339(s: string | null | undefined): number | null {
  if (!s) return null;
  const m = /^(\d{4}-\d{2}-\d{2})[Tt ](\d{2}:\d{2}:\d{2})(\.\d+)?([Zz]|[+-]\d{2}:?\d{2})$/.exec(s);
  if (!m) return null;
  const zone = m[4].toUpperCase() === "Z" ? "Z" : m[4].length === 5 ? `${m[4].slice(0, 3)}:${m[4].slice(3)}` : m[4];
  const ms = Date.parse(`${m[1]}T${m[2]}${zone}`);
  return Number.isNaN(ms) ? null : Math.floor(ms / 1000);
}

function isJustCreated(user: AuthUser, now: number): boolean {
  const created = unixFromRfc3339(user.created_at);
  return created !== null && now - created >= 0 && now - created < JUST_CREATED_SECS;
}

function createdByGoogleSignIn(user: AuthUser): boolean {
  const accountAt = unixFromRfc3339(user.created_at);
  const ids = user.identities;
  if (accountAt === null || !Array.isArray(ids) || ids.length !== 1) return false;
  const identityAt = unixFromRfc3339(ids[0].created_at);
  return ids[0].provider === "google" && identityAt !== null && Math.abs(identityAt - accountAt) <= SAME_MOMENT_SECS;
}

/** Worth one devices read? False for every account `verdict` would keep anyway. */
export function mayBeEmptyGoogleAccount(user: AuthUser, now: number): boolean {
  return isJustCreated(user, now) && createdByGoogleSignIn(user);
}

/** `devicePlatforms`: the platforms of the account's devices; one is enough to keep it. */
export function verdict(user: AuthUser, now: number, devicePlatforms: readonly string[]): Verdict {
  return mayBeEmptyGoogleAccount(user, now) && devicePlatforms.length === 0 ? "reject" : "keep";
}
