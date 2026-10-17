# Warden desktop — status

Updated 2026-10-17 (builder 1: foundation; builder 2: macOS drain, pill, shield, picker, quit, autostart). Staging build by default: "Warden β", `in.weenja.warden.desktop.staging`.

## Works
- Tauri v2 tray-only app: no Dock icon (`LSUIElement` + `ActivationPolicy::Accessory`), MOA template icon in the menu bar, popover under the icon (hides on blur), single instance, strict CSP, per-window capabilities (`src-tauri/capabilities/`).
- UI ported from `mockups/desktop/` with OS light/dark: sign-in (Google + email code; Apple hidden), "This Mac is linked", popover (balance, battery, guarded apps, sites, browsers, crew line, Quit). Alignment fixes are marked `fix:` in `ui/styles/ui.css`.
- Rust core: `api.rs` (typed requests, 20 s timeout), `auth.rs` (PKCE + `warden://auth-callback` deep link, email OTP), `session.rs` (keychain, refresh 2 min before expiry, single-flight, rotation saved before use), `sync.rs` (register_device, heartbeat every 60 s and on wake/unlock, block list, crew, balance-token decode, `spend_tick`, `set_guarded_apps`).
- `platform/macos.rs`: frontmost app, polite terminate, installed apps with 64 px PNG icons, screen locked, computer name. `platform/windows.rs` is a compiling skeleton.
- Quit sends a last heartbeat with `stopped_at` (5 s cap), then exits.

## Staging results (2026-10-17, fresh anonymous user each run)
- register macos + windows device: OK. Re-register with the same install id returns the same row.
- heartbeat macos: OK. heartbeat **windows: OK** (no attestation verifier; staging accepts it).
- set_guarded app on the Mac device: OK; spending on it from the Windows device: 400 "block target belongs to another device".
- dev-tools add_points 5 -> 300 s. spend-tick 60 s -> charged 60, 300 -> 240. Retry with the same evidence -> charged 0, balance 240.
- refresh rotation: r0 -> r1 -> r2 all distinct. Reusing r0 right away is still ACCEPTED (GoTrue reuse interval), so the app never relies on reuse failing.
- single-flight against staging: 8 callers on an expired session -> 1 refresh, 1 token.
- Deep link: the debug .app receives `open warden://auth-callback?code=...`; a link with no sign-in started is refused ("This sign-in link has expired").
- Google authorize: `/auth/v1/authorize?provider=google&redirect_to=warden://auth-callback&...` -> 302 to accounts.google.com (client 243483196176-qda4…, redirect_uri = the Supabase callback) -> 200 "Sign in - Google Accounts". No `redirect_uri_mismatch`, no `invalid_client`. The real click is for Jojo.

## Run
```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cd desktop && npm install
npm run tauri dev                       # tray icon + sign-in window (dev server on 127.0.0.1:1420)
npm run tauri build -- --debug --bundles app
open "src-tauri/target/debug/bundle/macos/Warden β.app"
```
Sign-in with Google needs the bundled app: macOS only routes `warden://` links to an installed .app bundle, not to `tauri dev`. Open the .app once (LaunchServices registers the scheme), click Continue with Google, finish in the browser, allow "Open Warden β".
Email codes need custom SMTP on Supabase first (staging has none, so codes only reach team addresses).

## Checks
```sh
cd desktop/src-tauri
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
PATH="/opt/homebrew/opt/llvm/bin:$PATH" cargo check --target x86_64-pc-windows-msvc   # llvm-rc (brew install llvm, done) for the Windows resource file
cargo test --test staging -- --ignored --nocapture                                  # staging, fresh anonymous user, deleted after
cd .. && npx tsc --noEmit
```
TLS: rustls on macOS; SChannel (native-tls) on Windows, because rustls' crypto (aws-lc / ring) needs a Windows C toolchain to cross-check.

## Drain, pill, shield (builder 2, macOS)
- `drain.rs`: pure rules mirroring Android GuardCore (14 unit tests): guarded app in front drains; a known browser drains whole while sites are guarded (tick with no target and no site, as Android); locked screen / sleep / nothing in front = no drain; local countdown from the balance token; ticks every 30 s of use, at once on target change and at zero; evidence `d<start ms>.<seq>`, reused on retry; refused 4xx dropped; low toast once at <=10 pts (re-arms at 12); lock at local 0, token expiry (offline) or a server shield with points left (daily max, until the day key changes).
- `guard.rs`: 1 s poll through `Platform`, gap > 2 s not counted, ticks in the background (retry after 5 s). Warden's own windows in front keep the app under them for 30 s (popover shows "Steam on this Mac"). Shield "Close" steps aside 10 s so the app can quit or ask to save.
- `placement/`: the only OS window code. macOS (`placement/macos.rs`): pill, shield, toast are NSPanels (tauri-nspanel 2.1): non-activating, all Spaces + full-screen apps. Pill level 27 (above the menu bar), centred on the notch (`auxiliaryTopLeft/RightArea`), height = `safeAreaInsets.top`; no notch: top centre of the menu bar. Shield: ModalPanel level, screen minus the menu bar, key window without activating Warden (the locked app gets no keys, stays frontmost). The screen is the one with the frontmost app's front window (CGWindowList bounds only, no permission). `pill()`, `shield()`, `toast()` also show the window; never call `show()` on them.
- Windows: pill, shield, toast, picker, quit pages + capabilities; popover "Edit" (picker) and Quit (asks first); "Open at login" row on the linked screen (tauri-plugin-autostart, LaunchAgent; turned on at sign-in in release builds).

## Staging results, drain loop (2026-10-17, fake Mac with a scripted front app, 10 s per poll)
- add_points 12 -> 720 s, multiplier 1. Steam 90 s -> 3 ticks, server 720 -> 630. Chrome 30 s with a guarded site -> whole-browser tick accepted, 630 -> 600.
- Low toast once at 650 s (10 pts). Steam to zero: 60 polls (600 s), shield (zero), pill locked, server balance 0.
- Finder in front: shield and pill gone, nothing charged. Steam again: shield back.

## Not tested on a screen (no screen-recording permission in the build shell)
Pill at the notch / no-notch screen, hover via the panel tracking area, panels over full-screen apps and on other Spaces, shield taking the keyboard, picker with real icons, login item. UI checked in headless Chrome (dark + light) with fake data only.

## Merge note (Windows branch)
`placement/mod.rs` here also has `toast()`, `pill_metrics()` / `PillMetrics`, the shared `popover_near_icon()` and a `generic` fallback (non-mac); keep them when merging builder 3's mod.rs, and keep `placement/macos.rs` as is.

## For builder 3 (Windows)
- Fill `src-tauri/src/platform/windows.rs` (the trait in `platform/mod.rs`). The tray/popover placement already flips above the icon when the tray is at the bottom.
- Deep link: `register_all()` runs in Windows debug builds; installers register `warden://` from `tauri.conf.json`.

## Left
- Prod config (`config/prod.json`, identifier `in.weenja.warden.desktop`) waits for Jojo's approval.
- Apple sign-in hidden until Apple is configured on Supabase.
- Signing/notarisation, DMG/NSIS release flow.
