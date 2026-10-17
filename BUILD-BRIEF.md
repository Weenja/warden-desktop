# Desktop build brief (shared by all builder agents)

Founder: Jojo. Approved 2026-10-17: Tauri v2, sign-in = normal device login, designs in `mockups/desktop/` ("mostly okay; fix alignment issues if any"). Build the desktop app first (Mac, then Windows). The extension comes later. Must work end to end on STAGING: sign in, sync, drain, stop, block.

## Product rules
- 1 point = 1 minute. Points are earned on the phone only. Guarded apps/sites drain points while they are in front. At 0 the shield blocks them.
- No onboarding, no dashboard. Sign in → "This Mac is linked" → app lives in the menu bar (Mac) / tray (Windows).
- Drain pill: Mac at the notch (no-notch screen: hangs from the top centre of the menu bar); Windows at the top centre. Hidden when nothing drains. Copy (same as Android): title "<label> · <pts> pts left", text "−<n>/min while it's open"; low (≤10 pts): "Low battery · −<n>/min · MOA locks it at 0". Mint normal, amber low, coral zero. Stop closes the app (tab via extension later).
- Shield at 0: "that's your lot." / "Out of points. Earn more to use <app>." / button "Close <app>".
- Guarded sites come from the phone (device-wide). Guarded apps are per computer (picker in the popover).
- A browser without the Warden extension drains the whole time it is in front while the user has guarded sites (mirror Android "whole browser" ticks, see GuardService.kt / Sites.kt). The extension is not built yet, so for now every browser counts as "no extension".
- Quit Warden = crew gets told (send heartbeat `stopped_at` before exit, like Android).

## Backend (closed repo `../warden-prd`, branch prd-gaps = what staging runs). READ ONLY. Do not edit, deploy or migrate it; if the backend needs a change, stop and report.
- Staging values: `../warden-prd/apps/android/local.properties` keys `STAGING_SUPABASE_URL`, `STAGING_SUPABASE_PUBLISHABLE_KEY` (public by design; copy into `desktop/src-tauri/config/staging.json`). Prod values come later (`WARDEN_SUPABASE_URL` + key) — leave a `prod.json` with the same shape only when Jojo approves prod.
- Do not use the Supabase Management API, admin/service keys or any secret. Do not change any project setting (staging or prod). The app and the tests use only the public URL + publishable key.
- Auth (staging, read 2026-10-17): Google enabled with secret (OAuth redirect flow works if Google allows the Supabase callback); Apple DISABLED (hide the Apple button until it is configured); email enabled but NO custom SMTP (email codes do not reach normal users; build the code flow, it works once SMTP exists). `uri_allow_list` = `warden://auth-callback` only.
- Sign-in flow: PKCE via `GET /auth/v1/authorize?provider=google&redirect_to=warden://auth-callback&code_challenge=…&code_challenge_method=s256` in the system browser (the redirect is already on the allowlist; PKCE makes a stolen code useless), deep link back into the app with tauri-plugin-deep-link (scheme `warden`), then `POST /auth/v1/token?grant_type=pkce`. Refresh with `grant_type=refresh_token` (rotation is on). Store the session in the OS keychain (`keyring` crate).
- Devices: RPC `register_device(p_platform 'macos'|'windows', p_name, p_install_id, p_push_token null, p_app_version)`; install_id = random UUID kept in app data.
- Edge functions (Bearer = user JWT, `apikey` = publishable key): `heartbeat` {device_id, blocking_enabled, screen_time_permission, health_permission, app_version?, stopped_at?} → {device, wallet, balance_token, balance_token_expires_at}; `spend-tick` {device_id, target_id | site, seconds 1..900, evidence (idempotency key)} → {charged_seconds, balance_seconds, multiplier, low_balance, shield, wallet, balance_token, …}. Balance token = HS256 JWT, 15 min TTL; read its payload locally, decrement between ticks, shield at local 0 or expiry. Server is authoritative.
- Block list: table `block_targets` (kind app|category|site, token, label, device_id null = all devices) via PostgREST under RLS; write with RPC `set_guarded(p_kind, p_items, p_device)`.
- Attestation: Mac has none (server caps macOS at log mode). Windows has no verifier; check what staging does for platform `windows` with a test device.
- Staging test helper: edge function `dev-tools` {action:"add_points", n:1..200} (acts on the caller).
- Android reference for behaviour (read only): `../warden-prd/apps/android/platform/src/main/java/app/warden/android/platform/{GuardService,Pills,LiveUpdates,Shield}.kt`, `.../core` (drain math, ticks, outbox).

## Testing without Jojo's Google login
- Staging allows anonymous sign-in (`POST /auth/v1/signup` with an empty body and the publishable key). Tests (`#[ignore]`, `cargo test -- --ignored`) use a fresh anonymous user. The app itself has no anonymous or password login.
- Google sign-in: verify the authorize redirect reaches Google without `redirect_uri_mismatch` / `invalid_client`; Jojo does the real click at the end.

## Layout
- `desktop/src-tauri/src/`: `main.rs`, `api.rs` (HTTP client, typed requests), `auth.rs`, `session.rs` (keychain), `sync.rs` (heartbeat, wallet, block list), `drain.rs` (pure drain logic + tests), `platform/{mod,macos,windows}.rs` (trait `Platform`: frontmost app, terminate, installed apps, screen locked), `windows.rs`/`ui` placement for pill, popover, shield.
- `desktop/ui/`: one page per window (signin, linked, popover, pill, shield, picker, quit), shared `ui.css` from `mockups/desktop/ui.css`.
- App ids: `in.weenja.warden.desktop` (prod, "Warden"), `in.weenja.warden.desktop.staging` ("Warden β"). Build staging by default.

Status and handoff notes go in `desktop/STATUS.md` (short).
