// HTTP client for the Warden API (Supabase): the only network peer of this extension, used in
// standalone mode only (linked to the desktop app, the extension makes no network calls).
//
// Three kinds of endpoint, all on the same host:
// - Auth (`/auth/v1/...`): sign-in and token refresh. Only the publishable key is sent.
// - Edge functions (`/functions/v1/...`): heartbeat and spend ticks. Need the user's token.
// - PostgREST (`/rest/v1/...`): RPCs and tables, guarded by row-level security on the server.
//
// Every field sent here is listed in extension/DATA.md. Tokens are never logged.

import { PUBLISHABLE_KEY, SUPABASE_URL } from "./config.ts";
import type { AuthUser } from "./new-account.ts";

/** A request that takes longer is treated as failed; the alarms retry on their own schedule. */
const TIMEOUT_MS = 20_000;

export class ApiError extends Error {
  /** HTTP status, or 0 when there was no answer (offline, DNS, timeout). */
  readonly status: number;
  readonly code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export type TokenResponse = {
  access_token: string;
  refresh_token: string;
  expires_at?: number;
  expires_in: number;
  user: AuthUser;
};

export type Wallet = {
  balance_seconds: number;
  low_balance: boolean;
  multiplier: number;
  day_key: string;
  daily_max_seconds?: number | null;
};

export type WalletAnswer = { wallet: Wallet; balance_token: string; balance_token_expires_at: string };
export type SpendAnswer = WalletAnswer & {
  charged_seconds: number;
  balance_seconds: number;
  low_balance: boolean;
  shield: boolean;
};
export type BlockTarget = { id: string; kind: string; token: string; label: string; device_id: string | null };
export type Platform = "macos" | "windows" | "linux" | "android";

async function send<T>(path: string, init: RequestInit, token?: string): Promise<T> {
  const headers: Record<string, string> = { apikey: PUBLISHABLE_KEY, "content-type": "application/json" };
  if (token) headers.authorization = `Bearer ${token}`;
  let res: Response;
  try {
    res = await fetch(`${SUPABASE_URL}${path}`, { ...init, headers, signal: AbortSignal.timeout(TIMEOUT_MS) });
  } catch (e) {
    throw new ApiError(0, "network", e instanceof Error ? e.message : String(e));
  }
  const text = await res.text();
  const body = text ? safeJson(text) : null;
  if (!res.ok) throw errorFrom(res.status, body, text);
  return body as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

/** The three backends shape errors differently; pull out a stable code and a message. */
function errorFrom(status: number, body: unknown, text: string): ApiError {
  const b = (body ?? {}) as Record<string, unknown>;
  const nested = (typeof b.error === "object" && b.error !== null ? b.error : {}) as Record<string, unknown>;
  const code = String(nested.code ?? b.error_code ?? b.code ?? (typeof b.error === "string" ? b.error : "unknown"));
  const message = String(nested.message ?? b.msg ?? b.message ?? b.error_description ?? text.slice(0, 200));
  return new ApiError(status, code, message);
}

const post = (body: unknown): RequestInit => ({ method: "POST", body: JSON.stringify(body) });

// ----- Auth -----

/** Swap the one-time code from the Google redirect for a session; the verifier proves we started it. */
export const exchangePkce = (authCode: string, verifier: string) =>
  send<TokenResponse>("/auth/v1/token?grant_type=pkce", post({ auth_code: authCode, code_verifier: verifier }));

/** New session from the refresh token. Rotation is on: the old refresh token stops working. */
export const refreshSession = (refreshToken: string) =>
  send<TokenResponse>("/auth/v1/token?grant_type=refresh_token", post({ refresh_token: refreshToken }));

/** Email a 6-digit code. `create_user: false`: accounts are made on the phone, not here. */
export const sendEmailCode = (email: string) => send<unknown>("/auth/v1/otp", post({ email, create_user: false }));

export const verifyEmailCode = (email: string, code: string) =>
  send<TokenResponse>("/auth/v1/verify", post({ type: "email", email, token: code }));

// ----- Device, wallet, block list -----

/** Create or update this browser's device row. The install id makes it idempotent. */
export const registerDevice = (token: string, platform: Platform, name: string, installId: string, version: string) =>
  send<{ id: string }>(
    "/rest/v1/rpc/register_device",
    post({ p_platform: platform, p_name: name, p_install_id: installId, p_push_token: null, p_app_version: version }),
    token,
  );

/** Tell the server this device is alive (the crew sees a silent one as "went dark") and get the wallet. */
export const heartbeat = (token: string, deviceId: string, version: string) =>
  send<WalletAnswer>(
    "/functions/v1/heartbeat",
    post({
      device_id: deviceId,
      blocking_enabled: true,
      // The phone's "usage access" flag. The extension sees the front tab without any extra
      // permission, so this is true; false would tell the crew it was revoked.
      screen_time_permission: true,
      health_permission: false,
      app_version: version,
    }),
    token,
  );

/** Charge seconds on a guarded site. The server picks the guarded site and is authoritative. */
export const spendTick = (token: string, deviceId: string, site: string, seconds: number, evidence: string) =>
  send<SpendAnswer>("/functions/v1/spend-tick", post({ device_id: deviceId, site, seconds, evidence }), token);

/** The user's guarded apps and sites. Row-level security returns only the caller's rows. */
export const blockTargets = (token: string) =>
  send<BlockTarget[]>("/rest/v1/block_targets?select=id,kind,token,label,device_id&order=created_at.asc", {}, token);

/** Up to one of the caller's devices. Read only after a Google sign-in made a brand-new account. */
export const anyDevice = (token: string) =>
  send<{ id: string; platform: string }[]>("/rest/v1/devices?select=id,platform&limit=1", {}, token);

/** Delete the caller's own account. Only for the empty account a Google sign-in just made. */
export const deleteAccount = (token: string) => send<unknown>("/functions/v1/delete-account", post({}), token);
