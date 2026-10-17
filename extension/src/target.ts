// Which browser this bundle is for, set by the build (scripts/build.mjs). Kept apart from
// config.ts so the unit tests can load config without a build.

declare const __BROWSER__: "chrome" | "firefox";
export const BROWSER: "chrome" | "firefox" = __BROWSER__;
