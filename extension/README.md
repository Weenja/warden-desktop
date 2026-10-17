# Warden browser extension (staging)

Chrome extension id (staging, fixed by the public `key` in `src/manifest.json`):

    blcmgpbfbbemkkcfjgbhgblnapbndfgp

Native messaging origin for the desktop host manifest: `chrome-extension://blcmgpbfbbemkkcfjgbhgblnapbndfgp/`
Firefox extension id (staging): `extension-staging@getwarden.app`
Native host name (staging): `in.weenja.warden.staging`

The private key that matches the id is kept outside the repo at `~/.config/warden/extension-staging-key.pem`
(only needed to pack a `.crx`; loading unpacked uses the public key in the manifest). The Chrome
Web Store refuses a `key` in uploads, so a store build must drop it (the store keeps the id).

What it does: see `EXTENSION-BRIEF.md` at the repo root. What it sends: [`DATA.md`](DATA.md).
- **Linked** (Warden desktop app on this computer): reports the front tab's site to the app over
  native messaging and shows the app's state; no network calls of its own.
- **Standalone** (no app: Firefox on Android, or a computer without the app): signs in and talks
  to the Warden API itself.

## Build

```sh
cd extension
npm install
npm run build        # dist/chrome and dist/firefox
npm run zip          # also dist/warden-{chrome,firefox}-<version>.zip
npm run check        # tsc --noEmit, unit tests (node --test), web-ext lint on dist/firefox
```

Node 22.18+ (the unit tests run the TypeScript sources directly). Icons: `scripts/make-icons.sh`
(ImageMagick) rebuilds `static/icons/` from the brand mark; the PNGs are committed.

## Install

### Chrome, Edge, Brave, Arc (load unpacked)
1. Open `chrome://extensions` (Edge: `edge://extensions`), turn on **Developer mode**.
2. **Load unpacked** → pick `extension/dist/chrome`.
3. The id must read `blcmgpbfbbemkkcfjgbhgblnapbndfgp`. A first-run tab opens once.
4. Pin the Warden icon. With the Warden desktop app running and signed in, the popup says
   "Linked to Warden on …"; without it, it offers sign-in (standalone).

Branded Chrome ignores `--load-extension`; for scripted tests use Chrome for Testing
(`npx @puppeteer/browsers install chrome@stable`) with puppeteer `enableExtensions`.

### Firefox desktop (temporary add-on)
1. Open `about:debugging#/runtime/this-firefox` → **Load Temporary Add-on…**
2. Pick `extension/dist/firefox/manifest.json`. It stays until Firefox quits.
   Or: `npx web-ext run -s dist/firefox` (opens a fresh profile with it).
3. Firefox 127+ shows host permissions at install; if the Supabase site access is off, the first
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
- **Signed unlisted .xpi (stays installed):** Firefox only installs signed add-ons. Needs an
  addons.mozilla.org account and its API keys (Tools → Manage API keys), then
  ```sh
  npx web-ext sign -s dist/firefox --channel=unlisted --api-key "$AMO_JWT_ISSUER" --api-secret "$AMO_JWT_SECRET"
  ```
  Copy the `.xpi` to the phone. In Firefox: Settings → About Firefox → tap the logo 5 times, then
  Settings → **Install extension from file** → pick the `.xpi`. Each new version needs a higher
  `version` in `src/manifest.json`.

## Status (2026-10-18)

Tested:
- `npm run check`: tsc clean, 17 unit tests (site match, drain countdown and evidence keys, lock
  rules, new-account verdict, PKCE + callback parsing), web-ext lint 0 errors / 0 warnings.
- Chrome for Testing 155 (puppeteer): id `blcmgpbfbbemkkcfjgbhgblnapbndfgp`, first-run tab opens,
  no host → standalone; popup, blocked and first-run pages render (light).
- Linked, with a fake native host in the test profile: hello → state; tab reports
  (`www.example.com`); app lock → open guarded tab sent to `blocked.html`, DNR rule on, badge "0",
  a new tab to the site is blocked.
- Standalone on staging, fresh anonymous user (deleted after): register (macos), heartbeat, block
  list, one `spend-tick {site: "example.com"}` for 46 s: server balance 300 → 254.
- Google capture on a real Supabase callback (the user-cancel redirect, a genuine
  `302 Location: warden://auth-callback?error=...`): Chrome observes the Location and moves the tab
  to the first-run page; Firefox 158 cancels the response (the link never reaches the OS), moves
  the tab, and shows the error. The code path uses the same parser; a real Google login is still
  needed for the end-to-end code exchange.
- Staging accepts an `android` device with no attestation (log mode).

Open:
- Prod: if `ATTEST_MODE_ANDROID=enforce`, Firefox on Android (platform `android`, no Play
  Integrity) will get `401 attestation_failed` on heartbeat and spend-tick.
- The battery needs today's max: standalone has it; linked shows it only if the app adds
  `daily_max` to `state` (optional field), else the battery is hidden.
- No public source URL yet, so "Read the code" links from the mockup are left out.
- Toolbar badge at 0 shows "0" on coral (a badge is text only; no lock glyph).
