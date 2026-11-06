# Warden for Mac, Windows and your browser

Warden makes you earn your screen time. You earn points on your phone (1 point = 1 minute); the
apps and sites you guard drain them while they are in front, and lock at 0.

This repo holds the two computer parts, open source so you can check what they do:

- **Desktop app** (`desktop/`, Tauri v2, macOS and Windows): lives in the menu bar / tray, drains
  points while a guarded app is in front, shows a small pill while it drains and a shield at 0.
- **Browser extension** (`extension/`, Chrome and Firefox): tells the desktop app which site is in
  front, so only guarded sites drain instead of the whole browser, and blocks them at 0. Without the
  desktop app (for example Firefox on Android) it works on its own.

Both need the **Warden phone app**: your account, your crew and your points live there. Sign in to
the phone app first, then sign in here with the same Google account. Get it at
[getwarden.app](https://getwarden.app).

What each part sends, field by field: [`DATA.md`](DATA.md) (desktop) and
[`extension/DATA.md`](extension/DATA.md). No analytics, no telemetry, no crash reporting.

## Install

Downloads are on the [Releases](https://github.com/GoldenGuts/warden-desktop/releases) page.

### Mac (macOS 12 or later)
1. Download `Warden_<version>_universal.dmg`, open it and drag Warden to Applications.
2. The app is not signed with an Apple Developer ID yet, so macOS blocks the first open. Either
   right-click Warden in Applications → **Open** → **Open**, or run once in Terminal:
   ```sh
   xattr -d com.apple.quarantine /Applications/Warden.app
   ```
   (On recent macOS, if there is no Open button: System Settings → Privacy & Security → **Open
   Anyway**.)
3. Warden shows a mochi in the menu bar (no Dock icon). Click **Continue with Google** and finish in
   your browser.

### Windows 10 and 11
1. Download `Warden_<version>_x64-setup.exe` and run it. It installs for your user only (no admin).
2. The installer is not code-signed yet, so Windows SmartScreen may say "Windows protected your
   PC". Click **More info** → **Run anyway**.
3. Warden shows in the tray. Click **Continue with Google** and finish in your browser.

### Chrome, Edge, Brave, Arc, Vivaldi
The extension is not in the Chrome Web Store yet, so it is loaded by hand:
1. Download `warden-chrome-<version>.zip` and unzip it into a folder you keep.
2. Open `chrome://extensions` (Edge: `edge://extensions`), turn on **Developer mode**.
3. **Load unpacked** and pick that folder. Pin the Warden icon.

### Firefox
Firefox installs only add-ons signed by Mozilla for good, and Warden is not on
addons.mozilla.org yet. To try it: open `about:debugging#/runtime/this-firefox` →
**Load Temporary Add-on…** → pick `warden-firefox-<version>.zip`. It stays until Firefox quits.
More (Firefox for Android, signed builds): [`extension/README.md`](extension/README.md).

## Known limits
- Sign-in is Google only, and the Warden account must already exist (it is made in the phone app).
  A Google account with no Warden account gets "No Warden account for …".
- The Mac app is unsigned and not notarised; the Windows installer is unsigned (see above).
- One Warden app per computer: the prod app ("Warden") and the test build ("Warden β") both open
  `warden://` sign-in links, so keep only one installed.

## Build from source

Needs Rust (stable) and Node 22.18+.

```sh
# Desktop app. Default: the "Warden β" staging build (test backend).
cd desktop
npm ci
npm run tauri dev                      # run it (sign-in needs a bundled app, see desktop/STATUS.md)
npm run tauri build                    # Warden β
npm run build:prod                     # Warden (prod backend, identifier, name and icons)

# Browser extension: prod and staging, Chrome and Firefox, folders and zips in extension/dist/
cd extension
npm ci
npm run build
npm run check
```

`npm run build:prod` passes `--config src-tauri/tauri.prod.conf.json` to Tauri. That file sets the
prod identifier, name and icons, and `build.rs` compiles in the prod backend
(`src-tauri/config/prod.json`), extension ids and native messaging host name when it sees it. The
app refuses to start if the two disagree.

Checks: see [`CLAUDE.md`](CLAUDE.md) ("Checks before done"). Releases: pushing a `v*` tag runs
`.github/workflows/release.yml` (Mac universal app, Windows installer, extension zips).

## Layout

| Path | What |
|---|---|
| `desktop/src-tauri/` | Rust core: sign-in, sync, drain rules, OS watchers, browser link |
| `desktop/ui/` | The app's windows (HTML/CSS/TypeScript) |
| `desktop/STATUS.md` | What works, what was tested how, what is open |
| `extension/` | The browser extension (MV3, TypeScript) |
| `mockups/` | The approved designs the UI is built from |
| `DATA.md` | Everything the desktop app sends |

## License

[AGPL-3.0](LICENSE). Fonts: Figtree and Quicksand, SIL Open Font License (`OFL-*.txt` next to
them).
