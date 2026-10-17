//! The link to the Warden browser extension, through the browser's native messaging.
//!
//! The browser starts this same binary as a "native messaging host" ([`host`]) when the
//! extension connects. The host only relays messages between the browser (stdin/stdout) and the
//! running Warden app (a local socket, [`endpoint`]). The app keeps one [`link::Link`] per
//! connected extension ([`server`], [`link`]).
//!
//! With the extension, Warden knows which SITE is open in the browser in front, so only guarded
//! sites drain (instead of the whole browser). The extension sends nothing but the active tab's
//! host name and whether the user is idle; it makes no network calls while linked.
//!
//! The browsers find the host through small JSON manifests written at every start ([`manifest`]).

pub mod endpoint;
pub mod frame;
pub mod host;
pub mod link;
pub mod manifest;
pub mod server;

/// Native messaging host name the extension connects to (staging build).
pub const HOST_NAME: &str = "in.weenja.warden.staging";
/// The staging Chrome extension's id, fixed by the public key in its manifest (see
/// `extension/README.md`). Only this extension may start the host in Chromium browsers.
pub const CHROME_EXTENSION_ID: &str = "blcmgpbfbbemkkcfjgbhgblnapbndfgp";
/// The staging Firefox add-on id. Only this add-on may start the host in Firefox.
pub const FIREFOX_EXTENSION_ID: &str = "extension-staging@getwarden.app";
/// The app's bundle id (macOS) / identifier, as in `tauri.conf.json`. The host starts the app
/// by it and finds the app's data folder with it (the host runs without Tauri).
pub const APP_ID: &str = "in.weenja.warden.desktop.staging";

/// Chrome's origin string for the extension, as used in `allowed_origins`.
pub fn chrome_origin() -> String {
    format!("chrome-extension://{CHROME_EXTENSION_ID}/")
}
