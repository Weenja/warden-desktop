# Warden for Mac, Windows and the browser

Warden turns screen time into something you earn. Points are earned on your phone (1 point = 1
minute). The apps and sites you guard spend them while they are in front, and lock when the balance
reaches zero.

This repository contains the two desktop components of Warden. They are open source so that anyone
can verify what runs on their computer.

- **Desktop app** (`desktop/`): a menu bar / system tray app for macOS and Windows. It spends points
  while a guarded app is in front, shows a small status pill while it does, and shows a shield when
  the balance is zero.
- **Browser extension** (`extension/`): for Chrome, Edge, Brave, Arc, Vivaldi and Firefox. It tells
  the desktop app which site is in front, so only guarded sites spend points instead of the whole
  browser, and blocks them at zero. On a device without the desktop app (for example Firefox for
  Android) it works on its own.

Both require the **Warden phone app**, where your account, crew and points live. Sign in on the
phone first, then sign in on the desktop with the same Google account. Get the phone app at
[getwarden.app](https://getwarden.app).

**Privacy.** Every request each component makes is documented field by field in [`DATA.md`](DATA.md)
(desktop app) and [`extension/DATA.md`](extension/DATA.md). There is no analytics, telemetry or
crash reporting.

## Install

Downloads are on the [Releases](https://github.com/Weenja/warden-desktop/releases) page.

### macOS 12 or later

1. Download `Warden_<version>_aarch64.dmg` (Apple silicon) or `Warden_<version>_x64.dmg` (Intel),
   open it and drag Warden to Applications.
2. This release is not yet notarised by Apple, so macOS blocks the first launch. Right-click
   Warden in Applications and choose **Open**, then **Open** again. On recent macOS versions,
   open System Settings → Privacy & Security and choose **Open Anyway** instead. Compare the
   file's SHA-256 with `SHA256SUMS.txt` on the release page if you want to verify the download.
3. Warden appears in the menu bar; there is no Dock icon. Choose **Continue with Google** and
   complete the sign-in in your browser.

### Windows 10 and 11

1. Download `Warden_<version>_x64-setup.exe` and run it. It installs for the current user and does
   not need administrator rights.
2. The installer does not yet carry a code-signing certificate, so Windows SmartScreen may show a
   warning. Choose **More info**, then **Run anyway**. Compare the file's SHA-256 with
   `SHA256SUMS.txt` on the release page if you want to verify the download.
3. Warden appears in the system tray. Choose **Continue with Google** and complete the sign-in in
   your browser.

### Chrome, Edge, Brave, Arc, Vivaldi

Until the extension is listed in the Chrome Web Store, it is installed from the release zip:

1. Download `warden-chrome-<version>.zip` and unzip it into a folder you will keep.
2. Open `chrome://extensions` (Edge: `edge://extensions`) and enable **Developer mode**.
3. Choose **Load unpacked** and select that folder. Pin the Warden icon to the toolbar.

### Firefox

Firefox permanently installs only add-ons signed by Mozilla. Until Warden is listed on
addons.mozilla.org, load it as a temporary add-on: open `about:debugging#/runtime/this-firefox`,
choose **Load Temporary Add-on…** and select `warden-firefox-<version>.zip`. It stays installed
until Firefox quits. See [`extension/README.md`](extension/README.md) for Firefox for Android.

## Current limitations

- Sign-in is with Google only, and the account must already exist in the phone app.
- The macOS app is not yet notarised and the Windows installer is not yet code-signed.
- The browser extension is not yet available from the Chrome Web Store or addons.mozilla.org.

## Build from source

Requirements: Rust (stable) and Node.js 22.18 or later.

```sh
# Desktop app
cd desktop
npm ci
npm run tauri dev          # development build against the test backend
npm run build:prod         # production build: Warden

# Browser extension (Chrome and Firefox builds and zips in extension/dist/)
cd extension
npm ci
npm run build
npm run check
```

`npm run build:prod` passes `src-tauri/tauri.prod.conf.json` to Tauri. That file sets the
production identifier, name and icons, and `build.rs` compiles in the production backend
configuration from `src-tauri/config/prod.json`. The app refuses to start if the two disagree.

Checks before a pull request: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test` and `npx tsc --noEmit`; for Windows code at least
`cargo check --target x86_64-pc-windows-msvc`. Pushing a `v*` tag runs
`.github/workflows/release.yml`, which builds the macOS apps (Apple silicon and Intel), the Windows
installer and the extension zips, and attaches them to a GitHub release.

## Repository layout

| Path | Contents |
|---|---|
| `desktop/src-tauri/` | Rust core: sign-in, sync, spending rules, OS integration, browser link |
| `desktop/ui/` | The app's windows (HTML, CSS, TypeScript) |
| `extension/` | The browser extension (Manifest V3, TypeScript) |
| `mockups/` | The design mockups the UI is built from |
| `DATA.md` | Every request the desktop app makes |

## License

[AGPL-3.0](LICENSE). Fonts: Figtree and Quicksand, SIL Open Font License (see the `OFL-*.txt`
files next to them).
