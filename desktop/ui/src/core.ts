// The UI's whole view of the Rust core: a few commands and one state event. No tokens ever
// reach the web views; they only get UiState and short, human error messages.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface GuardedItem { token: string; label: string }
export interface LinkedDevice { id: string; name: string }
export interface Browser { id: string; name: string; icon: string | null }

/** Mirrors `sync::UiState` in Rust. */
export interface UiState {
  signed_in: boolean;
  email: string | null;
  device: LinkedDevice | null;
  balance_seconds: number | null;
  daily_max_seconds: number | null;
  low_balance: boolean;
  guarded_apps: GuardedItem[];
  guarded_sites: number;
  crew: string[];
  browsers: Browser[];
  online: boolean;
}

export interface AuthResult { ok: boolean; message: string | null }

export const core = {
  getState: () => invoke<UiState>("get_state"),
  signInGoogle: () => invoke<void>("sign_in_google"),
  sendEmailCode: (email: string) => invoke<void>("send_email_code", { email }),
  verifyEmailCode: (email: string, code: string) => invoke<void>("verify_email_code", { email, code }),
  fitWindow: (height: number) => invoke<void>("fit_window", { height }),
  openSignin: () => invoke<void>("open_signin"),
  closeSignin: () => invoke<void>("close_signin"),
  quitWarden: () => invoke<void>("quit_warden"),
  onState: (fn: (s: UiState) => void): Promise<UnlistenFn> => listen<UiState>("state-changed", (e) => fn(e.payload)),
  onAuthResult: (fn: (r: AuthResult) => void): Promise<UnlistenFn> => listen<AuthResult>("auth-result", (e) => fn(e.payload)),
};

/** Commands reject with a plain string message from Rust. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : "Something went wrong. Try again.";
}
