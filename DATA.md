# What Warden desktop sends, and to whom

The only network peer is the Warden API (Supabase, `config/staging.json` in `desktop/src-tauri/`).
Sign-in with Google happens in your own browser; the app never sees your Google password.
No analytics, no telemetry, no crash reporting.

Every request carries the public `apikey` (the publishable key) and, once signed in, your
session token (`Authorization: Bearer ...`). Tokens are kept in the OS keychain only.

| When | Endpoint | Fields sent |
|---|---|---|
| Google sign-in (browser) | `GET /auth/v1/authorize` | `provider=google`, `redirect_to=warden://auth-callback`, `code_challenge` (SHA-256 of a random secret), `code_challenge_method=s256` |
| Google sign-in returns | `POST /auth/v1/token?grant_type=pkce` | `auth_code` (from the redirect), `code_verifier` (the random secret) |
| Email sign-in | `POST /auth/v1/otp` | `email`, `create_user: false` |
| Email code | `POST /auth/v1/verify` | `type: "email"`, `email`, `token` (the 6-digit code) |
| Token refresh (before expiry) | `POST /auth/v1/token?grant_type=refresh_token` | `refresh_token` |
| Sign-in and each start | `POST /rest/v1/rpc/register_device` | `p_platform` (`macos` / `windows`), `p_name` (the computer name, e.g. "Sam's MacBook Air"), `p_install_id` (random id made at install), `p_push_token: null`, `p_app_version` |
| Every 60 s, after wake/unlock, at Quit | `POST /functions/v1/heartbeat` | `device_id`, `blocking_enabled: true`, `screen_time_permission: true`, `health_permission: false`, `app_version`, `stopped_at` (only at Quit: the time you quit, so your crew is told) |
| While a guarded app is in front | `POST /functions/v1/spend-tick` | `device_id`, `target_id` (the guarded app's block-list id) or `site` (a guarded host), `seconds`, `evidence` (a one-time key so a retry is never charged twice) |
| Every 60 s | `GET /rest/v1/block_targets` | nothing (reads your guarded apps and sites) |
| Picking guarded apps | `POST /rest/v1/rpc/set_guarded` | `p_kind: "app"`, `p_items` (app id + name, e.g. `com.valvesoftware.steam` / "Steam"), `p_device` |
| Popover crew line | `POST /rest/v1/rpc/crew_state` | nothing (reads your crew's first names) |

Not sent: window titles, screen contents, URLs, which non-guarded apps you use, your installed
apps list (the picker reads it locally; only the apps you pick are sent).
