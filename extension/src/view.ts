// What the popup, first-run page and blocked page render. The background writes it to
// `storage.session` (key "view") on every change; pages read it and listen for changes.
// Never contains tokens.

import { ext } from "./ext.ts";

export type Mode = "checking" | "linked" | "standalone";

export type SigninView = {
  busy: boolean;
  error: string | null;
  /** Google sign-in found no Warden account for this email (the empty account was removed). */
  noAccountEmail: string | null;
  /** An email code was sent to this address; the popup asks for the code. */
  codeSentTo: string | null;
};

export type View = {
  mode: Mode;
  /** Linked: the desktop app is signed in. Standalone: this extension is signed in. */
  signedIn: boolean;
  /** Standalone: the account's email. */
  account: string | null;
  /** Linked: the desktop app's computer name. */
  deviceName: string | null;
  points: number | null;
  /** Today's earning cap in points, for the battery. Null = unknown (battery hidden). */
  dailyMax: number | null;
  low: boolean;
  locked: boolean;
  perMin: number;
  /** The front tab's site, and the guarded site that covers it (if any). */
  site: string | null;
  guardedSite: string | null;
  draining: boolean;
  guardedSites: string[];
  /** Standalone: the in-page points chip is on. */
  chip: boolean;
  signin: SigninView;
};

export const NO_SIGNIN: SigninView = { busy: false, error: null, noAccountEmail: null, codeSentTo: null };

export const EMPTY_VIEW: View = {
  mode: "checking",
  signedIn: false,
  account: null,
  deviceName: null,
  points: null,
  dailyMax: null,
  low: false,
  locked: false,
  perMin: 1,
  site: null,
  guardedSite: null,
  draining: false,
  guardedSites: [],
  chip: false,
  signin: NO_SIGNIN,
};

export async function readView(): Promise<View> {
  const got = await ext.storage.session.get("view");
  return (got.view as View | undefined) ?? EMPTY_VIEW;
}

/** Commands pages send to the background with `runtime.sendMessage`. */
export type Command =
  | { cmd: "google"; selectAccount: boolean }
  | { cmd: "email_send"; email: string }
  | { cmd: "email_verify"; code: string }
  | { cmd: "signin_reset" }
  | { cmd: "sign_out" }
  /** Close the front tab (popup "Stop · close tab" / "Close tab"). */
  | { cmd: "stop" }
  /** Close the tab that sent this (blocked page). */
  | { cmd: "close_me" }
  | { cmd: "show_app" }
  | { cmd: "chip"; on: boolean };
