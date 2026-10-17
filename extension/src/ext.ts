// One name for the extension API in both browsers. Firefox has always had the promise-based
// `browser` namespace; Chrome has it from version 148 and returns promises from `chrome.*` in
// Manifest V3 before that. The `chrome` types describe both well enough for what we use.

declare const browser: typeof chrome | undefined;

export const ext: typeof chrome = typeof browser !== "undefined" ? browser : chrome;
