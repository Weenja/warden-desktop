# Warden desktop — status

Updated 2026-10-18 (browser extension link); 2026-10-17 (builder 1: foundation; builder 2: macOS drain, pill, shield, picker, quit, autostart). Staging build by default: "Warden β", `in.weenja.warden.desktop.staging`.

**Before the first prod build:** follow "Desktop app: Google sign-in on prod" in the private repo `warden/RELEASING.md` (prod Google secret + `https://getwarden.app/auth` on the prod allow-list, `config/prod.json` with `auth_return_url`, one app per `warden://` scheme, click test).

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
- Deep link: the debug .app receives `open warden://auth-callback?code=...`; a link with no sign-in started is refused ("This sign-in link has expired"), unless the app is already signed in or the code is the one it just used (the web page's "Open Warden" button): then it only shows the popover / sign-in window.
- Google authorize: `/auth/v1/authorize?provider=google&redirect_to=warden://auth-callback&...` -> 302 to accounts.google.com (client 243483196176-qda4…, redirect_uri = the Supabase callback) -> 200 "Sign in - Google Accounts". No `redirect_uri_mismatch`, no `invalid_client`. The real click is for Jojo.

## Google sign-in with no Warden account (2026-10-18)
Accounts are made on the phone. Google sign-in makes Supabase create an empty account for an unknown Google account, so right after the PKCE exchange and before anything is saved or registered, `new_account.rs` checks: `user.created_at` older than 10 min = keep (no request); also keep unless the account has exactly one identity, Google, whose `created_at` is within 5 s of `user.created_at` (a phone account starts anonymous and links Google later); else one `devices` read (any device = keep); else delete it with `delete-account`, clear the session, and show "No Warden account for <email>" ("Use another account" = authorize with `prompt=select_account`; a QR code for getwarden.app, from `scripts/make-qr.py`, with a link that opens the site). Invariant: delete only when ALL hold (young, no device, one Google identity made with the account); any missing or unreadable field keeps. If the devices read or the delete fails, nothing is saved and an error shows; a retry within 10 min checks again.
- Staging: anonymous user, no devices, as returned (no identities) -> Kept; same user with a same-moment Google identity -> Deleted; its refresh token then fails (400 refresh_token_not_found). `prompt=select_account` reaches the Google redirect (checked with curl).
- The identity rule also protects a phone account (anonymous first, Google linked later) whose phone has not registered a device yet. Left edge: a phone user who signed up WITH Google under 10 min ago and whose phone has no device yet (register failed) would be deleted. Not tested with a real Google click yet.
- Mockup: "No account" figure added (Mac + Windows); mockup `ui.css` button labels now centred like the app (only the label span grows, "G" `flex: none`).

## Run
```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cd desktop && npm install
npm run tauri dev                       # tray icon + sign-in window (dev server on 127.0.0.1:1420)
npm run tauri build -- --debug --bundles app
open "src-tauri/target/debug/bundle/macos/Warden β.app"
```
Sign-in with Google needs the bundled app: macOS only routes `warden://` links to an installed .app bundle, not to `tauri dev`. Open the .app once (LaunchServices registers the scheme), click Continue with Google, finish in the browser. Since 2026-10-18 Supabase returns the browser to `https://staging.getwarden.app/auth` (config `auth_return_url`, on the staging redirect allow-list); that page opens `warden://auth-callback` and shows "You're signed in" (before, the tab stayed dimmed on Google's page). Allow "Open Warden β" once (tick "Always allow").
- 2026-10-18: Google sign-in had failed with "Unable to exchange external code" because the staging project held a wrong Google client secret (log: `invalid_client`); fixed in the staging auth config.
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

## Windows (builder 3, branch `windows`)
Done, compiles (`cargo check` + `clippy -D warnings` for x86_64-pc-windows-msvc) and links (`cargo xwin build --target x86_64-pc-windows-msvc` -> debug `warden.exe` + `warden-native-host.exe`; cargo-xwin downloads the MSVC CRT + Windows SDK and accepts Microsoft's licence without a prompt). NOT run on Windows yet:
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

## Browser extension link (2026-10-18, `src/browser_ext/`)
Native messaging per EXTENSION-BRIEF. Host name `in.weenja.warden.staging`; Chrome id `blcmgpbfbbemkkcfjgbhgblnapbndfgp` (from `extension/README.md`), Firefox `extension-staging@getwarden.app` (constants in `browser_ext/mod.rs`).
- Host = a separate console program `warden-native-host` (`src/bin/warden-native-host.rs`, runs `browser_ext::host::run()`, no Tauri), on macOS and Windows alike. Tauri bundles every bin of the crate next to the app (`Contents/MacOS/` in the .app; `$INSTDIR` in the NSIS installer, removed on uninstall), so no sidecar setup is needed; the manifests point at it. Why console: Chrome on Windows starts hosts through `cmd /c` with piped stdio, which a GUI-subsystem exe may not keep. The GUI `main.rs` has no host branch. `tauri dev` builds only the app bin: run `cargo build --bins` once for the helper. It relays frames stdin/stdout <-> the app's local endpoint (macOS `~/Library/Application Support/in.weenja.warden.desktop.staging/ext.sock`, mode 0600; Windows `\\.\pipe\warden-staging-ext-<domain>-<user>`, first instance only, no remote clients). App not reachable: `open -g -b <bundle id>` (Windows: `warden.exe` beside the helper, detached, breakaway from the browser's job if allowed), retry every 1 s for 15 s, then 5 s; the last `hello` + `tab` are replayed after every (re)connect. Exits when the browser closes stdin.
- Anti-tamper: the app accepts a connection only from the exact `warden-native-host` file beside its own exe (peer pid: `LOCAL_PEERPID` / `GetNamedPipeClientProcessId`, paths resolved; not even the app exe itself) and finds the browser itself by walking the host's parents to the first `KNOWN_BROWSERS` app (Windows: through cmd.exe). The host's word is not trusted. Started by hand from a terminal = linked with no browser: gets hello + state, never changes the drain. The Windows host also checks the pipe server is `warden.exe` beside it (anti-squatting).
- Manifests at every start (`browser_ext/manifest.rs`, rewritten only when changed): macOS for each installed browser folder (Chrome, Chrome Beta, ChromeForTesting, Chromium, Edge, Brave, Arc, Vivaldi; Firefox -> `Mozilla/`); Windows two JSON files in the app data folder + HKCU keys for Chrome, Edge, Brave, Chromium, Vivaldi, Mozilla.
- Drain (`drain.rs`): browser with a linked extension -> drains only when its last tab is on a guarded site (equal or subdomain, most specific) and not idle; kind `site`, label = the guarded site, tick `spend-tick {site: host}`. No extension -> whole browser as before. Several links for one browser (Chrome profiles): the most recent tab report wins. Locked on a site: no shield (the extension shows its blocked page); `close_tab` if still open after 3 s; pill Stop = `close_tab`. `state` goes to every extension on each change (deduped). Popover browsers row: "Linked" / "No extension". "Open Warden" (`show_app`) opens the popover, or sign-in when signed out.

Tested 2026-10-18:
- Unit: frame codec, host-launch args, replay, link registry (hello/state/dedupe/per-browser close_tab), site match, browser drain choice, manifests (allowed ids, idempotent write).
- Staging (`extension_site_drain_and_whole_browser`): fake Mac, Chrome in front, fake extension on `m.youtube.com` with `youtube.com` guarded: 300 -> 270 s; ledger row target = the youtube.com block target. example.com and idle: nothing charged; server refuses `site: example.com` (404). Extension gone: whole-browser tick 270 -> 240, ledger target null.
- Real host round trip on this Mac: (first version, host inside the app binary) with framed stdin got `hello` + `state` from the running app; with the app quit, the host started it (`open -b`) and still got both. A plain Python client on the socket is refused (closed, no reply). Manifests verified on disk for Chrome, Chromium, Edge, Brave, Arc, Vivaldi, ChromeForTesting (no Firefox installed here).

Console helper (2026-10-18, later): fmt, clippy -D warnings (mac + x86_64-pc-windows-msvc), 69 tests, tsc pass. Real round trip: the .app's `Contents/MacOS/warden-native-host` with framed stdin got `hello` + `state`; with the app quit it started it and got both; a copy of the helper outside the .app (`target/debug/warden-native-host`) and a Python client are refused. PE check: `warden-native-host.exe` CONSOLE, `warden.exe` GUI; installer.nsi installs both.

Builds (2026-10-18, rebuilt with the helper):
- Mac debug app rebuilt: `src-tauri/target/debug/bundle/macos/Warden β.app` (running on this Mac; it wrote the manifests above).
- Windows NSIS installer (release, unsigned, per user): `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Warden β_0.1.0_x64-setup.exe` (3.8 MB). Build: `brew install nsis`, then `LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 PATH="/opt/homebrew/opt/llvm/bin:$PATH" npm run tauri build -- --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis`. makensis 3.13 crashes (`std::bad_alloc`) when no locale is set, so the locale variables are required.
- Regression: the old drain staging test still passes (Steam 3 ticks, whole-browser tick).

Needs Jojo:
- Mac: load the extension in real Chrome, open youtube.com with a guarded YouTube: pill says "youtube.com · N pts left", popover shows Chrome "Linked"; Stop closes the tab; at 0 the blocked page shows (no shield over Chrome). Firefox not installed here (Mozilla manifest untested).
- Windows: check the extension links (popover "Linked") through the console helper `warden-native-host.exe` (Chrome -> cmd.exe -> helper; the app should name Chrome as the browser).

## Left
- Prod config (`config/prod.json`, identifier `in.weenja.warden.desktop`) waits for Jojo's approval.
- Apple sign-in hidden until Apple is configured on Supabase.
- Signing/notarisation, DMG/NSIS release flow.
