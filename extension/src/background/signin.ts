// Standalone sign-in: Google (OAuth + PKCE in a normal tab) or a 6-digit email code.
//
// Why the Google redirect is read from a response header: the Supabase project allows only
// `warden://auth-callback` as a redirect, and Firefox on Android has no `identity` API. So the
// sign-in runs in a normal tab, and when Supabase's own `/auth/v1/callback` answers with
// `Location: warden://auth-callback?code=...`, `webRequest.onHeadersReceived` reads the code
// (host permission: the Supabase origin only). Firefox cancels that response so the link never
// reaches the OS (where the Warden phone app could try to open it); Chrome cannot block in
// Manifest V3, so it only observes, and we move the tab on at once. PKCE makes a code that
// anyone else sees useless: the verifier never leaves this extension.

import * as api from "../api.ts";
import { ApiError } from "../api.ts";
import { authorizeUrl, challengeFor, looksLikeEmail, newVerifier, parseCallback } from "../auth.ts";
import { SUPABASE_URL } from "../config.ts";
import { BROWSER } from "../target.ts";
import { ext } from "../ext.ts";
import { mayBeEmptyGoogleAccount, verdict } from "../new-account.ts";
import { NO_SIGNIN, type SigninView } from "../view.ts";
import { saveSession, sessionFrom } from "./session.ts";

/** A browser sign-in older than this is abandoned; its verifier is dropped. */
const PENDING_TTL_MS = 10 * 60_000;
export const CALLBACK_URL_PATTERN = `${SUPABASE_URL}/auth/v1/callback*`;

type Pending = { verifier: string; startedAt: number };

/** Called after a new session is saved (main.ts starts standalone sync). */
let afterSignIn: () => Promise<void> = async () => {};
export function onSignedIn(hook: () => Promise<void>): void {
  afterSignIn = hook;
}

export async function readSignin(): Promise<SigninView> {
  const got = await ext.storage.session.get("signin");
  return (got.signin as SigninView | undefined) ?? NO_SIGNIN;
}

async function setSignin(change: Partial<SigninView>): Promise<void> {
  await ext.storage.session.set({ signin: { ...(await readSignin()), ...change } });
}

export const resetSignin = () => ext.storage.session.set({ signin: NO_SIGNIN });

function message(e: unknown): string {
  if (e instanceof ApiError) return e.status === 0 ? "No connection. Try again." : e.message;
  return e instanceof Error ? e.message : String(e);
}

/** Open Google sign-in in a new tab. Starting again replaces the pending verifier. */
export async function startGoogle(selectAccount: boolean): Promise<void> {
  const verifier = newVerifier();
  const pending: Pending = { verifier, startedAt: Date.now() };
  await ext.storage.session.set({ pending });
  await setSignin({ ...NO_SIGNIN, busy: true });
  await ext.tabs.create({ url: authorizeUrl(SUPABASE_URL, await challengeFor(verifier), selectAccount) });
}

/** `webRequest.onHeadersReceived` for the Supabase callback. */
export function onCallbackHeaders(
  details: chrome.webRequest.OnHeadersReceivedDetails,
): chrome.webRequest.BlockingResponse | undefined {
  const location = details.responseHeaders?.find((h) => h.name.toLowerCase() === "location")?.value;
  const result = location ? parseCallback(location) : null;
  if (!result) return undefined;
  void finishGoogle(details.tabId, result);
  return BROWSER === "firefox" ? { cancel: true } : undefined;
}

async function finishGoogle(tabId: number, result: { code: string } | { error: string }): Promise<void> {
  // Show the result on the first-run page, which shows the sign-in state.
  if (tabId >= 0) await ext.tabs.update(tabId, { url: ext.runtime.getURL("welcome.html") }).catch(() => undefined);
  const got = await ext.storage.session.get("pending");
  await ext.storage.session.remove("pending");
  const pending = got.pending as Pending | undefined;
  if ("error" in result) {
    await setSignin({ busy: false, error: result.error });
    return;
  }
  if (!pending || Date.now() - pending.startedAt > PENDING_TTL_MS) {
    await setSignin({ busy: false, error: "This sign-in link has expired. Try again." });
    return;
  }
  try {
    const tokens = await api.exchangePkce(result.code, pending.verifier);
    if (await isEmptyNewAccount(tokens)) {
      // The empty account this sign-in just made (accounts are made on the phone): removed.
      await api.deleteAccount(tokens.access_token);
      await setSignin({ busy: false, noAccountEmail: tokens.user.email ?? "this Google account" });
      return;
    }
    await saveSession(sessionFrom(tokens));
    await resetSignin();
    await afterSignIn();
  } catch (e) {
    await setSignin({ busy: false, error: message(e) });
  }
}

/** See new-account.ts. At most one read; any doubt keeps the account. */
async function isEmptyNewAccount(tokens: api.TokenResponse): Promise<boolean> {
  const now = Math.floor(Date.now() / 1000);
  if (!mayBeEmptyGoogleAccount(tokens.user, now)) return false;
  const devices = await api.anyDevice(tokens.access_token);
  return verdict(tokens.user, now, devices.map((d) => d.platform)) === "reject";
}

export async function sendCode(email: string): Promise<void> {
  const address = email.trim();
  if (!looksLikeEmail(address)) {
    await setSignin({ error: "That does not look like an email address." });
    return;
  }
  await setSignin({ ...NO_SIGNIN, busy: true });
  try {
    await api.sendEmailCode(address);
    await setSignin({ busy: false, codeSentTo: address });
  } catch (e) {
    await setSignin({ busy: false, error: message(e) });
  }
}

export async function verifyCode(code: string): Promise<void> {
  const { codeSentTo } = await readSignin();
  if (!codeSentTo) return;
  await setSignin({ busy: true, error: null });
  try {
    await saveSession(sessionFrom(await api.verifyEmailCode(codeSentTo, code.trim())));
    await resetSignin();
    await afterSignIn();
  } catch (e) {
    await setSignin({ busy: false, error: message(e) });
  }
}
