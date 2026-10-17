# Warden desktop — status

Updated 2026-10-17 (builder 1: foundation). Staging build by default: "Warden β", `in.weenja.warden.desktop.staging`.

## Works
- Tauri v2 tray-only app: no Dock icon (`LSUIElement` + `ActivationPolicy::Accessory`), MOA template icon in the menu bar, popover under the icon (hides on blur), single instance, strict CSP, per-window capabilities (`src-tauri/capabilities/`).
- UI ported from `mockups/desktop/` with OS light/dark: sign-in (Google + email code; Apple hidden), "This Mac is linked", popover (balance, battery, guarded apps, sites, browsers, crew line, Quit). Alignment fixes are marked `fix:` in `ui/styles/ui.css`.
- Rust core: `api.rs` (typed requests, 20 s timeout), `auth.rs` (PKCE + `warden://auth-callback` deep link, email OTP), `session.rs` (keychain, refresh 2 min before expiry, single-flight, rotation saved before use), `sync.rs` (register_device, heartbeat every 60 s and on wake/unlock, block list, crew, balance-token decode, `spend_tick`, `set_guarded_apps`).
- `platform/macos.rs`: frontmost app, polite terminate, installed apps with 64 px PNG icons, screen locked, computer name. `platform/windows.rs`: see the Windows section.
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

## For builder 2 (drain, pill, shield, picker, quit, autostart)
- Add `drain.rs` (pure logic + tests) and a drain task started in `lib.rs::start`. Inputs: `state.sync.platform().frontmost_app()`, `.screen_locked()`, `sync.guarded_apps()` (token = bundle id, `id` = target id), `sync.guarded_sites()` + `platform::KNOWN_BROWSERS` (no extension yet: a known browser in front drains while sites are guarded), `sync.balance()` (claims: balance_seconds, exp). Charge with `sync.spend_tick(&SpendTarget::Target(id) | Site(host), seconds, evidence)`; reuse the evidence on retry.
- Extend `sync::UiState` (+ `ui/src/core.ts`) with what is draining; the popover's "Nothing draining" row (`#now`) and `#mood` are ready for it.
- Picker: `sync.platform().installed_apps()` (slow, off the main thread) and `sync.set_guarded_apps(&[GuardedItem])`. Add an "Edit" link to the popover's "Guarded on this Mac" head (design has it).
- Quit confirm: `quit_warden` already tells the crew; put the confirm window in front of it.
- Autostart: add the "Open at login" row to `linked.html` (in the mockup) when it works.
- New window = new page in `ui/`, new input in `vite.config.ts`, a capability file, and its commands in `build.rs`.

## Windows (builder 3, branch `windows`)
Done, compiles (`cargo check` + `clippy -D warnings` for x86_64-pc-windows-msvc) and links (`cargo xwin build --target x86_64-pc-windows-msvc` -> debug `warden-desktop.exe`; cargo-xwin downloads the MSVC CRT + Windows SDK and accepts Microsoft's licence without a prompt). NOT run on Windows yet:
- `platform/windows.rs`: front app = foreground window's exe (`steam.exe`, lowercase), name = exe FileDescription, else the exe name; Store apps resolved through ApplicationFrameHost's child window; Warden's own windows count as nothing in front. `terminate_app`: WM_CLOSE to the app's visible top-level windows, then TerminateProcess after 5 s (known browsers are only asked: tabs). Lock = WTS session flag. Name = GetComputerNameExW (DNS host name).
- `platform/windows/apps.rs` + `icon.rs`: picker = Start-menu shortcuts (all users + this user; Squirrel `Update.exe --processStart X.exe` → `x.exe`) then Uninstall keys' DisplayIcon exe; installers/uninstallers dropped; deduped by exe; 64 px PNG icons (shortcut icon location, else the exe).
- `platform::KNOWN_BROWSERS` is per OS now (`MACOS_BROWSERS`, `WINDOWS_BROWSERS`).
- `placement/{mod,windows,macos,taskbar}.rs`: the shared interface. Windows `pill()` also SHOWS the pill (a plain `show()` would take focus): not focusable + tool window + topmost, top centre of the primary work area; create the pill window `transparent(true)`, `decorations(false)`, `focused(false)`. `shield()` covers the work area of the foreground window's monitor (taskbar stays). `popover()` opens from whichever edge the taskbar is on. `placement/macos.rs` is a stand-in: keep main's at merge.
- Tray: colour MOA-blue mochi (`icons/tray-color.png`, staging; prod needs the cream mochi). Installer: NSIS per user (`installMode: currentUser`, no admin), WebView2 bootstrapper; `warden://` is registered by the installer from the deep-link config. Autostart with tauri-plugin-autostart writes HKCU Run on Windows, so it is per user too (nothing Windows-specific to add).

Needs a real Windows PC:
- Front app ids for: Steam, Chrome/Edge, a Store app (Netflix/Photos), an elevated app (should still work: limited query rights).
- Close: Notepad with unsaved text (WM_CLOSE asks; ended after 5 s), a tray-only app, a browser (asked only).
- Picker list and icons look right (Office "advertised" shortcuts may be missing; check Discord/Slack ids).
- Lock state flips on Win+L; device name matches Settings > About.
- Pill: does not take focus from the app in front, not in Alt+Tab, no taskbar button, transparent corners. Popover with the taskbar on each edge and at 150 %/200 % scaling. Shield leaves the taskbar visible.
- NSIS installer: installs without admin, `warden://auth-callback` opens the running app (single instance), Google sign-in end to end.

## Left
- Prod config (`config/prod.json`, identifier `in.weenja.warden.desktop`) waits for Jojo's approval.
- Apple sign-in hidden until Apple is configured on Supabase.
- Signing/notarisation, DMG/NSIS release flow.
