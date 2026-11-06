# What Warden desktop sends, and to whom

The only network peer is the Warden API (Supabase: `config/prod.json` in `desktop/src-tauri/` for
Warden, `config/staging.json` for the Warden β test build).
Sign-in with Google happens in your own browser, on Google's page (the app only opens it); the app never
sees your Google password.
No analytics, no telemetry, no crash reporting.

Every request carries the public `apikey` (the publishable key) and, once signed in, your
session token (`Authorization: Bearer ...`). The refresh token is kept in the OS keychain; the
access token (valid one hour) is kept in memory only, and a new one is fetched when the app starts.

| When | Endpoint | Fields sent |
|---|---|---|
| Google sign-in (browser, on Google's page, not the Warden API) | `GET https://accounts.google.com/o/oauth2/v2/auth` | `client_id` (Warden's public Google client), `redirect_uri=https://getwarden.app/auth` (staging: `staging.getwarden.app/auth`; that page opens `warden://auth-callback` with the ID token from the URL fragment), `response_type=id_token`, `scope=openid email profile`, `nonce` (SHA-256 of a random secret) |
| Google sign-in returns | `POST /auth/v1/token?grant_type=id_token` | `provider=google`, `id_token` (from Google: your Google account id, email and name, signed by Google), `nonce` (the random secret) |
| Google sign-in, only if the account is under 10 minutes old and its only identity is the Google one, made with the account (`user.created_at`, `user.identities` from the sign-in) | `GET /rest/v1/devices?select=id,platform&limit=1` | nothing (reads whether the account has any device; you only see your own) |
| Google sign-in, only if that account is under 10 minutes old, has no device, and has exactly one identity (Google, made within 5 s of the account): the empty account Google sign-in just made (accounts are made on the phone) | `POST /functions/v1/delete-account` | nothing (deletes the signed-in account itself; never called for an account that has a device, is older than 10 minutes, or linked Google after it was made) |
| "Use another account" (after "No Warden account") | Google's page, as above | same as Google sign-in, plus `prompt=select_account` (Google shows its account chooser) |
| Email sign-in | `POST /auth/v1/otp` | `email`, `create_user: false` |
| Email code | `POST /auth/v1/verify` | `type: "email"`, `email`, `token` (the 6-digit code) |
| Token refresh (before expiry) | `POST /auth/v1/token?grant_type=refresh_token` | `refresh_token` |
| Sign-in and each start | `POST /rest/v1/rpc/register_device` | `p_platform` (`macos` / `windows`), `p_name` (the computer name, e.g. "Sam's MacBook Air"), `p_install_id` (random id made at install), `p_push_token: null`, `p_app_version` |
| Every 60 s, after wake/unlock, at Quit | `POST /functions/v1/heartbeat` | `device_id`, `blocking_enabled: true`, `screen_time_permission: true`, `health_permission: false`, `app_version`, `stopped_at` (only at Quit: the time you quit, so your crew is told) |
| While a guarded app is in front (every 30 s of use, when you switch away, at 0) | `POST /functions/v1/spend-tick` | `device_id`, `target_id` (the guarded app's block-list id), `seconds` (time in front since the last tick), `evidence` (a one-time key like `d1792257961000.4`: start time of this run + tick number, so a retry is never charged twice) |
| While a browser WITH the Warden extension is in front and its active tab is on a guarded site (every 30 s of use, when you switch away, at 0) | `POST /functions/v1/spend-tick` | `device_id`, `site` (only the host name of that tab, e.g. `m.youtube.com`, and only when it is one of your guarded sites or under one), `seconds`, `evidence`. No path, no full URL, nothing about other tabs or sites you visit that are not guarded |
| While a browser WITHOUT the Warden extension is in front and you have guarded sites | `POST /functions/v1/spend-tick` | `device_id`, `seconds`, `evidence`. No `target_id`, no `site`, no URL: Warden cannot see which site is open, so the whole browser drains (same request as Android) |
| Every 60 s | `GET /rest/v1/block_targets` | nothing (reads your guarded apps and sites) |
| Picking guarded apps | `POST /rest/v1/rpc/set_guarded` | `p_kind: "app"`, `p_items` (app id + name, e.g. `com.valvesoftware.steam` / "Steam"), `p_device` |
| Popover crew line | `POST /rest/v1/rpc/crew_state` | nothing (reads your crew's first names) |

## On this computer only (never sent to the server)
- **Browser extension link.** When the Warden extension is installed, your browser starts this same
  app as a "native messaging host" and it relays messages to the running app over a local socket
  (macOS: a file only your user can open, in `~/Library/Application Support/in.weenja.warden.desktop/`;
  Windows: a named pipe for your user only, no network clients). The extension sends only the host name of the
  active tab (e.g. `m.youtube.com`, or nothing for non-web pages) and whether you are idle. The app sends
  back your points, whether guarded sites are locked, your guarded sites list, what drains, and this
  computer's name, so the extension can show its badge and blocked page. The app checks that the other end
  is its own host program started by a browser; nothing else can join.
- **Host manifests.** At each start the app writes small JSON files that tell your browsers where the host
  program is and which extension may start it: macOS `~/Library/Application Support/<browser>/NativeMessagingHosts/in.weenja.warden.json`
  (only for browsers you have; Firefox and Zen share `Mozilla`); Windows two files in the app data folder plus a per-user registry key per browser
  (`HKEY_CURRENT_USER\Software\<browser>\NativeMessagingHosts\in.weenja.warden`). The Warden β test build uses
  the same places with `.staging` names (`in.weenja.warden.desktop.staging`, `in.weenja.warden.staging`).
- **Which apps are browsers.** So that every browser drains while you have guarded sites, the app checks
  which apps can show web sites: on macOS it reads each app's `Info.plist` (does it open web links and HTML
  files); on Windows it reads the list of installed browsers that Settings > Default apps uses
  (`Clients\StartMenuInternet` in the registry) and the name built into each exe file.

"Get Warden for your phone" opens https://getwarden.app in your browser, and "Get extension" next to a browser
opens that browser's extension page there (`https://getwarden.app/chrome`, `/edge`, `/firefox`, `/safari`) in that
browser; the app itself sends nothing there.

Not sent: window titles, screen contents, full URLs or paths, sites you visit that are not guarded, window positions (read locally to put the pill and shield on the right screen), which non-guarded apps you use, your installed
apps list (the picker reads it locally; only the apps you pick are sent).
