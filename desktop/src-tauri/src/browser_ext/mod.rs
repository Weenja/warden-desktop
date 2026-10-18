//! The link to the Warden browser extension, through the browser's native messaging.
//!
//! The browser starts a small helper program, `warden-native-host` (`src/bin/`), as the
//! "native messaging host" ([`host`]) when the extension connects. The helper only relays
//! messages between the browser (stdin/stdout) and the running Warden app (a local socket,
//! [`endpoint`]). The app keeps one [`link::Link`] per
//! connected extension ([`server`], [`link`]).
//!
//! With the extension, Warden knows which SITE is open in the browser in front, so only guarded
//! sites drain (instead of the whole browser). The extension sends nothing but the active tab's
//! host name and whether the user is idle; it makes no network calls while linked.
//!
//! The browsers find the host through small JSON manifests written at every start ([`manifest`]).

pub mod download;
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

/// The app's exe name, `mainBinaryName` in `tauri.conf.json` (and the Cargo bin name).
pub const APP_EXE: &str = "warden";
/// The native messaging helper's exe name (`src/bin/warden-native-host.rs`). A console
/// program, so that Chrome on Windows, which starts hosts through `cmd /c` with pipes for
/// stdin/stdout, can rely on it keeping them (a GUI-subsystem app may not).
pub const HOST_EXE: &str = "warden-native-host";

/// Chrome's origin string for the extension, as used in `allowed_origins`.
pub fn chrome_origin() -> String {
    format!("chrome-extension://{CHROME_EXTENSION_ID}/")
}
