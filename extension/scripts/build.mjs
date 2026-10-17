// Builds dist/chrome and dist/firefox from one codebase (`npm run build`), and zips them with
// `--zip`. The two differ only in the manifest:
// - Chrome: a service worker, and the public `key` that fixes the extension id (README.md).
// - Firefox: an event page (`background.scripts`), the gecko id (desktop + Android), and
//   `webRequestBlocking` so the sign-in redirect can be stopped inside the browser.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "src");
const FIREFOX_ID = "extension-staging@getwarden.app";
// data_collection_permissions needs Firefox 140 (desktop) / 142 (Android).
const FIREFOX_MIN = "140.0";
const FIREFOX_ANDROID_MIN = "142.0";

const entries = {
  background: "background/main.ts",
  popup: "pages/popup.ts",
  welcome: "pages/welcome.ts",
  blocked: "pages/blocked.ts",
  chip: "content/chip.ts",
};

function manifestFor(browser) {
  const m = JSON.parse(readFileSync(join(src, "manifest.json"), "utf8"));
  if (browser === "chrome") return m;
  delete m.key;
  delete m.minimum_chrome_version;
  m.background = { scripts: ["background.js"] };
  m.permissions.push("webRequestBlocking");
  m.browser_specific_settings = {
    gecko: {
      id: FIREFOX_ID,
      strict_min_version: FIREFOX_MIN,
      // What Firefox shows at install. Standalone mode sends the guarded site in the front tab
      // and the time spent on it (browsing activity) and signs in with an email (identity).
      data_collection_permissions: { required: ["browsingActivity", "authenticationInfo", "personallyIdentifyingInfo"] },
    },
    gecko_android: { strict_min_version: FIREFOX_ANDROID_MIN },
  };
  return m;
}

async function build(browser) {
  const out = join(root, "dist", browser);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });

  await esbuild.build({
    entryPoints: Object.fromEntries(Object.entries(entries).map(([name, file]) => [name, join(src, file)])),
    outdir: out,
    bundle: true,
    format: "iife",
    target: browser === "chrome" ? "chrome120" : "firefox140",
    define: { __BROWSER__: JSON.stringify(browser) },
    loader: { ".webp": "dataurl" },
    legalComments: "none",
    logLevel: "warning",
  });

  const sprite = readFileSync(join(src, "pages", "sprite.svg"), "utf8");
  for (const file of readdirSync(join(src, "pages"))) {
    if (file.endsWith(".html")) {
      const html = readFileSync(join(src, "pages", file), "utf8").replace("<!-- sprite -->", sprite.trim());
      writeFileSync(join(out, file), html);
    } else if (file.endsWith(".css")) {
      cpSync(join(src, "pages", file), join(out, file));
    }
  }
  for (const dir of ["fonts", "mochi", "icons"]) cpSync(join(root, "static", dir), join(out, dir), { recursive: true });
  cpSync(join(root, "..", "LICENSE"), join(out, "LICENSE"));
  writeFileSync(join(out, "manifest.json"), JSON.stringify(manifestFor(browser), null, 2) + "\n");
  return out;
}

const version = JSON.parse(readFileSync(join(src, "manifest.json"), "utf8")).version;
for (const browser of ["chrome", "firefox"]) {
  const out = await build(browser);
  if (process.argv.includes("--zip")) {
    const zip = join(root, "dist", `warden-${browser}-${version}.zip`);
    rmSync(zip, { force: true });
    execFileSync("zip", ["-qr", zip, "."], { cwd: out });
    console.log(zip);
  } else {
    console.log(out);
  }
}
