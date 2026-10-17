# What the Warden extension sends, and to whom

No analytics, no telemetry, no crash reporting, no third-party code. The extension never reads
page content, full links (paths, queries), your history or what you type. It reads only the
hostname of the front tab (`m.youtube.com`), whether a browser window has focus, and whether you
are away (screen locked, or no input for 5 minutes while the tab plays no audio).

## Linked mode (the Warden desktop app is on this computer)

The extension talks only to the Warden app on this computer, over the browser's native messaging
(a local pipe; nothing leaves the computer from the extension). It makes **no network calls**.
The app sends what the root `DATA.md` lists (it now includes the guarded site's host in
`spend-tick`).

| When | Message to the app | Fields |
|---|---|---|
| Connect | `hello` | `ext_version` |
| Front tab changes (tab, site, window, away) | `tab` | `site` (lowercase host, or `null` for non-web pages), `idle` (true/false) |
| You press "Open Warden" | `show_app` | nothing |

## Standalone mode (no desktop app: Firefox on Android, or a computer without the app)

The only network peer is the Warden API (Supabase, `src/config.ts`). Every request carries the
public `apikey` (the publishable key) and, once signed in, your session token
(`Authorization: Bearer ...`).

Where the session is kept: `storage.local` of this browser profile. An extension cannot use the
OS keychain; only this extension can read its storage. Signing out deletes it.

| When | Endpoint | Fields sent |
|---|---|---|
| Google sign-in (a normal tab) | `GET /auth/v1/authorize` | `provider=google`, `redirect_to=warden://auth-callback`, `code_challenge` (SHA-256 of a random secret), `code_challenge_method=s256`; "Use another account" adds `prompt=select_account` |
| Google sign-in returns | `POST /auth/v1/token?grant_type=pkce` | `auth_code` (read from Supabase's redirect), `code_verifier` (the random secret) |
| Google sign-in, only if the account is under 10 minutes old and its only identity is the Google one, made with the account | `GET /rest/v1/devices?select=id,platform&limit=1` | nothing (reads whether the account has any device; you only see your own) |
| Google sign-in, only if that account is under 10 minutes old, has no device, and has exactly one identity (Google, made within 5 s of the account): the empty account the sign-in just made (accounts are made on the phone) | `POST /functions/v1/delete-account` | nothing (deletes the signed-in account itself) |
| Email sign-in | `POST /auth/v1/otp` | `email`, `create_user: false` |
| Email code | `POST /auth/v1/verify` | `type: "email"`, `email`, `token` (the 6-digit code) |
| Token refresh (before expiry) | `POST /auth/v1/token?grant_type=refresh_token` | `refresh_token` |
| After sign-in | `POST /rest/v1/rpc/register_device` | `p_platform` (`macos` / `windows` / `linux` / `android`, from the browser's platform info), `p_name` (e.g. "Firefox on Android"), `p_install_id` (random id made at install), `p_push_token: null`, `p_app_version` |
| Every 60 s while signed in | `POST /functions/v1/heartbeat` | `device_id`, `blocking_enabled: true`, `screen_time_permission: true`, `health_permission: false`, `app_version` |
| Every 60 s while signed in | `GET /rest/v1/block_targets` | nothing (reads your guarded sites) |
| While a guarded site is in the front tab and you are not away (every 30 s of use, when you switch away, at 0) | `POST /functions/v1/spend-tick` | `device_id`, `site` (the guarded site that covers the tab, e.g. `youtube.com` for `m.youtube.com`), `seconds` (time on it since the last tick), `evidence` (a one-time key like `x1792257961000.4`, so a retry is never charged twice) |

"Get Warden for your phone" opens https://getwarden.app in a new tab; the extension sends nothing there.

## Kept only in this browser

- The points chip (off by default) gets its numbers from the extension's storage and runs only
  on guarded sites you allowed. It sends nothing.
- At 0 points, a browser rule blocks the guarded sites and their tabs show the extension's
  blocked page. The rule is removed when points come back.
