// The UI's whole view of the Rust core: a few commands and one state event. No tokens ever
// reach the web views; they only get UiState and short, human error messages.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface GuardedItem { token: string; label: string }
export interface GuardedAppView { token: string; label: string; icon: string | null }
export interface LinkedDevice { id: string; name: string }
/** An installed browser; `extension`: the Warden extension is linked in it. */
export interface Browser { id: string; name: string; icon: string | null; extension: boolean }
/** "site": a guarded site in a browser with the Warden extension (label = the site). */
export type DrainKind = "app" | "browser" | "site";
/** Mirrors `guard::DrainView`: what the pill shows. */
export interface DrainView {
  app_id: string;
  label: string;
  kind: DrainKind;
  icon: string | null;
  balance_seconds: number;
  per_minute: number;
  low: boolean;
  locked: boolean;
}
/** Mirrors `guard::ShieldView`. */
export interface ShieldView { app_id: string; label: string; kind: DrainKind; reason: "zero" | "offline" | "daily_max" }
export interface PickerApp { id: string; name: string; icon: string | null; guarded: boolean; common: boolean }
export interface PillMetrics { notch_width: number; bar_height: number }

/** Mirrors `sync::UiState` in Rust. */
export interface UiState {
  signed_in: boolean;
  email: string | null;
  device: LinkedDevice | null;
  balance_seconds: number | null;
  daily_max_seconds: number | null;
  low_balance: boolean;
  guarded_apps: GuardedAppView[];
  guarded_sites: number;
  crew: string[];
  browsers: Browser[];
  online: boolean;
  drain: DrainView | null;
  shield: ShieldView | null;
}

/** `no_account`: the Google account has no Warden account (its email, "" if unknown). */
export interface AuthResult { ok: boolean; message: string | null; no_account: string | null }

export const core = {
  getState: () => invoke<UiState>("get_state"),
  /** `selectAccount`: Google shows its account chooser ("Use another account"). */
  signInGoogle: (selectAccount = false) => invoke<void>("sign_in_google", { selectAccount }),
  openGetWarden: () => invoke<void>("open_get_warden"),
  sendEmailCode: (email: string) => invoke<void>("send_email_code", { email }),
  verifyEmailCode: (email: string, code: string) => invoke<void>("verify_email_code", { email, code }),
  fitWindow: (height: number, viewport: number) => invoke<void>("fit_window", { height, viewport }),
  openSignin: () => invoke<void>("open_signin"),
  closeSignin: () => invoke<void>("close_signin"),
  quitWarden: () => invoke<void>("quit_warden"),
  pillFit: (width: number, height: number) => invoke<void>("pill_fit", { width, height }),
  pillMetrics: () => invoke<PillMetrics>("pill_metrics"),
  stopDraining: () => invoke<void>("stop_draining"),
  closeLockedApp: () => invoke<void>("close_locked_app"),
  listApps: () => invoke<PickerApp[]>("list_apps"),
  saveGuardedApps: (items: GuardedItem[]) => invoke<void>("save_guarded_apps", { items }),
  openPicker: () => invoke<void>("open_picker"),
  openQuit: () => invoke<void>("open_quit"),
  closeWindow: () => invoke<void>("close_window"),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (on: boolean) => invoke<boolean>("set_autostart", { on }),
  onState: (fn: (s: UiState) => void): Promise<UnlistenFn> => listen<UiState>("state-changed", (e) => fn(e.payload)),
  onPillHover: (fn: (inside: boolean) => void): Promise<UnlistenFn> => listen<boolean>("pill-hover", (e) => fn(e.payload)),
  onAuthResult: (fn: (r: AuthResult) => void): Promise<UnlistenFn> => listen<AuthResult>("auth-result", (e) => fn(e.payload)),
};

/** Commands reject with a plain string message from Rust. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : "Something went wrong. Try again.";
}
