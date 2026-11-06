// Builds the extension from one codebase (`npm run build`) for two browsers and two flavors,
// and zips each one:
//   dist/chrome, dist/firefox                  "Warden"   (prod backend)
//   dist/staging-chrome, dist/staging-firefox  "Warden β" (staging backend, for testing)
// The builds differ only in the manifest and the backend in src/config.ts:
// - Chrome: a service worker, and the public `key` that fixes the extension id (README.md), so
//   the desktop app can allow exactly this extension to start its native messaging host.
// - Firefox: an event page (`background.scripts`), the gecko id (desktop + Android), and
//   `webRequestBlocking` so the sign-in redirect can be stopped inside the browser.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as esbuild from "esbuild";
import { BACKENDS } from "../src/config.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "src");
// data_collection_permissions needs Firefox 140 (desktop) / 142 (Android).
const FIREFOX_MIN = "140.0";
const FIREFOX_ANDROID_MIN = "142.0";

// Public keys only: each fixes the Chrome extension id (README.md). The private keys are not in
// the repo; they are only needed to pack a .crx.
const FLAVORS = {
  prod: {
    name: "Warden",
    dir: "",
    chromeKey:
      "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAuj5h5JgrL09z+geNtMl8CHQSl2NiMhUH+AVNfL90dN2p6PMCvvYdf67l8l5DXIv80p3sYQe2vsu2iXZ1WC4OdoqJsGQzjil6Qvp1q7iacWquSu48NMUB+YTV+b3WG/iJsBpz/LdgvqlzJ/I9AYERKbB59DOQe8of3zJ7WfPS5SO5RVW/YgGtrhn0SCMn3+8Ze/i7wOBdz7NuUlD7StbZzQXNVIL4teHTqLi8uYAx5Xa/8VRxlRKN2bB0Xt+6GlEUdpr+zVuAFswAJrMjQBsbt1u602ZuJtIBUDjBzMTBAVvwrenUJujsJfGAabiGGNWxSwX9jNATvHU3G7FmNaVIMQIDAQAB",
    firefoxId: "extension@getwarden.app",
  },
  staging: {
    name: "Warden β",
    dir: "staging-",
    chromeKey:
      "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAyqOxFpnBTE+8rmo1x9JD6jlVgiX/dYlKdS5vx+zZVzcgnI1lrX62wvmroLs8DqLSL59BnvQZfcyyq1X9M2e5nCmJFJJ5VQ2rgpUW00NkM76e2SbmPYfSsE1/ISNRPxwYZJHS/c3msUSeQYNPcOFOhK4JUkFGn7UlaB9WdI/Hgl5k6oa3xrp+1K01Nl//Ab+ssamahLWO/xS6rpR1Gtl8zWl7VudnB1cu5yAe5ye1w25SgA0TqJi/uIvpzSVBaHtWWFtMk1LySc5folQ8zSWwBzfTaNfIJRzwVtbphsT6VH1L1a5DRSAd9SxeodPaP+UMdUeztSxxkaXTwfbXZFazPQIDAQAB",
    firefoxId: "extension-staging@getwarden.app",
  },
};

const entries = {
  background: "background/main.ts",
  popup: "pages/popup.ts",
  welcome: "pages/welcome.ts",
  blocked: "pages/blocked.ts",
  chip: "content/chip.ts",
};

function manifestFor(browser, flavor) {
  const f = FLAVORS[flavor];
  const m = JSON.parse(readFileSync(join(src, "manifest.json"), "utf8"));
  m.name = f.name;
  m.short_name = f.name;
  // The only site the extension talks to on its own (standalone mode): the Warden API.
  m.host_permissions = [`${BACKENDS[flavor].supabaseUrl}/*`];
  if (browser === "chrome") return { ...m, key: f.chromeKey };
  delete m.minimum_chrome_version;
  m.background = { scripts: ["background.js"] };
  m.permissions.push("webRequestBlocking");
  m.browser_specific_settings = {
    gecko: {
      id: f.firefoxId,
      strict_min_version: FIREFOX_MIN,
      // What Firefox shows at install. Standalone mode sends the guarded site in the front tab
      // and the time spent on it (browsing activity) and signs in with an email (identity).
      data_collection_permissions: { required: ["browsingActivity", "authenticationInfo", "personallyIdentifyingInfo"] },
    },
    gecko_android: { strict_min_version: FIREFOX_ANDROID_MIN },
  };
  return m;
}

async function build(browser, flavor) {
  const out = join(root, "dist", FLAVORS[flavor].dir + browser);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });

  await esbuild.build({
    entryPoints: Object.fromEntries(Object.entries(entries).map(([name, file]) => [name, join(src, file)])),
    outdir: out,
    bundle: true,
    format: "iife",
    target: browser === "chrome" ? "chrome120" : "firefox140",
    define: { __BROWSER__: JSON.stringify(browser), __WARDEN_ENV__: JSON.stringify(flavor) },
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
  writeFileSync(join(out, "manifest.json"), JSON.stringify(manifestFor(browser, flavor), null, 2) + "\n");
  return out;
}

const version = JSON.parse(readFileSync(join(src, "manifest.json"), "utf8")).version;
for (const flavor of Object.keys(FLAVORS)) {
  for (const browser of ["chrome", "firefox"]) {
    const out = await build(browser, flavor);
    const zip = join(root, "dist", `warden-${FLAVORS[flavor].dir}${browser}-${version}.zip`);
    rmSync(zip, { force: true });
    execFileSync("zip", ["-qr", zip, "."], { cwd: out });
    console.log(zip);
  }
}
