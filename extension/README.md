# Warden browser extension

Guarded sites drain the points you earn in the Warden phone app, and lock at 0. Chrome (and
Chromium browsers: Edge, Brave, Arc, Vivaldi) and Firefox (desktop and Android).
What it sends: [`DATA.md`](DATA.md).

- **Linked** (the Warden desktop app is on this computer): reports the front tab's site to the app
  over native messaging and shows the app's state; no network calls of its own.
- **Standalone** (no app: Firefox on Android, or a computer without the app): signs in and talks
  to the Warden API itself.

## Two builds

| | Warden (prod) | Warden β (staging, for testing) |
|---|---|---|
| Backend | the Warden API | the staging API |
| Folder after `npm run build` | `dist/chrome`, `dist/firefox` | `dist/staging-chrome`, `dist/staging-firefox` |
| Zip | `dist/warden-{chrome,firefox}-<version>.zip` | `dist/warden-staging-{chrome,firefox}-<version>.zip` |
| Chrome extension id | `popenmclimjlnklklpmmfhenbcdfmbjb` | `blcmgpbfbbemkkcfjgbhgblnapbndfgp` |
| Firefox add-on id | `extension@getwarden.app` | `extension-staging@getwarden.app` |
| Native messaging host | `in.weenja.warden` | `in.weenja.warden.staging` |
| Pairs with desktop app | Warden | Warden β |

The Chrome ids are fixed by the public `key` each Chrome manifest gets in `scripts/build.mjs`
(the desktop app allows only these ids to start its host). The private keys are not in this repo;
they are only needed to pack a `.crx`. The Chrome Web Store refuses a `key` in uploads, so a store
build must drop it (the store keeps its own id, which the desktop app would then have to allow).

## Build

```sh
cd extension
npm ci
npm run build        # all four folders and zips (see above)
npm run check        # tsc --noEmit, unit tests (node --test), web-ext lint on both Firefox builds
```

Node 22.18+ (the unit tests and the build script run TypeScript sources directly). Icons:
`scripts/make-icons.sh` (ImageMagick) rebuilds `static/icons/` from the brand mark; the PNGs are
committed.

## Install

### Chrome, Edge, Brave, Arc, Vivaldi (load unpacked)
1. Unzip `warden-chrome-<version>.zip` into a folder you keep (the browser loads it from there).
2. Open `chrome://extensions` (Edge: `edge://extensions`), turn on **Developer mode**.
3. **Load unpacked** and pick that folder (from source: `extension/dist/chrome`).
4. The id must read `popenmclimjlnklklpmmfhenbcdfmbjb`. A first-run tab opens once.
5. Pin the Warden icon. With the Warden desktop app running and signed in, the popup says
   "Linked to Warden on …"; without it, it offers sign-in (standalone).

### Firefox desktop (temporary add-on)
Firefox installs only add-ons signed by Mozilla for good. Until Warden is on addons.mozilla.org:
1. Open `about:debugging#/runtime/this-firefox` → **Load Temporary Add-on…**
2. Pick `warden-firefox-<version>.zip` (or `extension/dist/firefox/manifest.json`). It stays until
   Firefox quits. Or: `npx web-ext run -s dist/firefox` (opens a fresh profile with it).
3. Firefox shows host permissions at install; if the Warden API site access is off, the first
   "Continue with Google" asks for it.

### Firefox for Android
Standalone mode only (Android has no native messaging). Needs Firefox for Android 142+.
- **Test build over USB (temporary):** install Android platform tools (`adb` in `PATH`), turn on
  USB debugging on the phone, and in Firefox: Settings → **Remote debugging via USB**. Check
  `adb devices`, then:
  ```sh
  npx web-ext run -t firefox-android -s dist/firefox --adb-device <id> --firefox-apk org.mozilla.firefox
  ```
  (`org.mozilla.firefox_beta` for Beta, `org.mozilla.fenix` for Nightly.) It loads into the
  phone's main profile until Firefox restarts.
- **Signed unlisted .xpi (stays installed):** needs an addons.mozilla.org account and its API keys
  (Tools → Manage API keys), then
  ```sh
  npx web-ext sign -s dist/firefox --channel=unlisted --api-key "$AMO_JWT_ISSUER" --api-secret "$AMO_JWT_SECRET"
  ```
  Copy the `.xpi` to the phone. In Firefox: Settings → About Firefox → tap the logo 5 times, then
  Settings → **Install extension from file** → pick the `.xpi`. Each new version needs a higher
  `version` in `src/manifest.json`.

## Native messaging (linked mode)

The browser starts the desktop app's helper `warden-native-host` when the extension calls
`runtime.connectNative(<host>)`; the helper relays to the running app. Messages are JSON, framed by
the browser with a 4-byte length. Types: `src/protocol.ts`.

Extension → app:
- `{"type":"hello","ext_version":"0.1.0"}` first.
- `{"type":"tab","site":"m.youtube.com"|null,"idle":false}` on every change: `site` is the
  lowercase host of the active tab in the last focused window (`null` for non-web pages); `idle`
  is a locked screen, or no input for 5 minutes while the tab plays no audio.
- `{"type":"show_app"}` when you press "Open Warden".

App → extension:
- `{"type":"hello","app_version":"…"}`.
- `{"type":"state","signed_in":true,"points":42,"low":false,"locked":false,"guarded_sites":["youtube.com"],"draining":{"site":"youtube.com","per_min":1}|null,"device_name":"…"}`
  after hello and on every change. `locked` = guarded sites are blocked (0 points, the balance
  token expired offline, or today's max spent).
- `{"type":"close_tab","site":"youtube.com"}`: the pill's Stop, or a lock while a guarded site is open.

Mode: if `connectNative` works and the host answers `hello`, the extension is linked; "host not
found" means standalone. It checks again at browser start and every 5 minutes while standalone.
Site match (both sides): a host is guarded when it equals a guarded site or ends with "." + it
(`m.youtube.com` is under `youtube.com`). The server decides the charge.

The app trusts only its own helper, started by a browser (it checks the connecting process), and
the browser lets only the ids above start the helper (the host manifests' allowed lists).

## Known limits

- Firefox on Android: if the server enforces Android attestation, a standalone extension (platform
  `android`, no Play Integrity) gets `401 attestation_failed` on heartbeat and spend-tick.
- The battery needs today's max: standalone has it; linked shows it only when the app sends
  `daily_max` in `state`, else the battery is hidden.
- The toolbar badge at 0 shows "0" on coral (a badge is text only; no lock glyph).
