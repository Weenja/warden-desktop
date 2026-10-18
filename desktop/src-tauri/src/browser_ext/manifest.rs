//! Native messaging host manifests: small JSON files that tell each browser which program to
//! start when the Warden extension calls `runtime.connectNative("in.weenja.warden.staging")`,
//! and which extension may do so. Written at every app start (idempotent, per user, no admin),
//! so they always point at the helper next to the app that runs now (`warden-native-host`, the
//! same on macOS and Windows), even after the app moved or updated.
//!
//! Locations (per user), from the Chrome and MDN native messaging docs:
//! - macOS: `~/Library/Application Support/<browser folder>/NativeMessagingHosts/<name>.json`,
//!   only for browsers whose folder exists (we do not create folders for browsers you do not
//!   have); Firefox, and Zen with it, reads `~/Library/Application Support/Mozilla/NativeMessagingHosts/`.
//! - Windows: the JSON files live in Warden's app data folder; a registry key per browser under
//!   `HKEY_CURRENT_USER\Software\<browser>\NativeMessagingHosts\<name>` points at them.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{chrome_origin, FIREFOX_EXTENSION_ID, HOST_NAME};

const DESCRIPTION: &str = "Warden: tells the Warden app which site is open, so only guarded sites drain";

/// Chrome and Chromium browsers: only the Warden extension's origin may start the host.
pub fn chrome_manifest(exe: &Path) -> Value {
    json!({
        "name": HOST_NAME,
        "description": DESCRIPTION,
        "path": exe,
        "type": "stdio",
        "allowed_origins": [chrome_origin()],
    })
}

/// Firefox: only the Warden add-on may start the host.
pub fn firefox_manifest(exe: &Path) -> Value {
    json!({
        "name": HOST_NAME,
        "description": DESCRIPTION,
        "path": exe,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_EXTENSION_ID],
    })
}

/// Write `manifest` to `path` unless it already says exactly that.
fn write_if_changed(path: &Path, manifest: &Value) -> io::Result<()> {
    let text = serde_json::to_string_pretty(manifest)?;
    if std::fs::read_to_string(path).ok().as_deref() == Some(text.as_str()) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

/// macOS: one file per browser folder, named after the host.
#[cfg(any(target_os = "macos", test))]
fn file_name() -> String {
    format!("{HOST_NAME}.json")
}

/// macOS browser folders under `~/Library/Application Support`: (folder that shows the browser
/// is installed, folder that holds its NativeMessagingHosts, is Firefox).
#[cfg(target_os = "macos")]
const MAC_BROWSERS: &[(&str, &str, bool)] = &[
    ("Google/Chrome", "Google/Chrome", false),
    ("Google/Chrome Beta", "Google/Chrome Beta", false),
    ("Google/ChromeForTesting", "Google/ChromeForTesting", false),
    ("Chromium", "Chromium", false),
    ("Microsoft Edge", "Microsoft Edge", false),
    ("BraveSoftware/Brave-Browser", "BraveSoftware/Brave-Browser", false),
    ("Arc/User Data", "Arc/User Data", false),
    ("Vivaldi", "Vivaldi", false),
    ("Firefox", "Mozilla", true),
    // Zen is a Firefox fork and keeps Firefox's folder for hosts (Firefox source:
    // toolkit/xre/nsXREDirProvider.cpp, XRE_USER_NATIVE_MANIFESTS).
    ("zen", "Mozilla", true),
];

/// Write the manifests for every installed browser. Returns the files that now point at `exe`.
#[cfg(target_os = "macos")]
pub fn register(exe: &Path) -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else { return Vec::new() };
    let support = PathBuf::from(home).join("Library/Application Support");
    let mut written = Vec::new();
    for (installed, folder, firefox) in MAC_BROWSERS {
        if !support.join(installed).is_dir() {
            continue;
        }
        let path = support.join(folder).join("NativeMessagingHosts").join(file_name());
        // Firefox and Zen share one file.
        if written.contains(&path) {
            continue;
        }
        let manifest = if *firefox { firefox_manifest(exe) } else { chrome_manifest(exe) };
        match write_if_changed(&path, &manifest) {
            Ok(()) => written.push(path),
            Err(e) => log::warn!("host manifest {}: {e}", path.display()),
        }
    }
    written
}

/// Registry keys (under HKEY_CURRENT_USER) each browser reads: (key, is Firefox).
#[cfg(windows)]
const WINDOWS_BROWSERS: &[(&str, bool)] = &[
    (r"Software\Google\Chrome", false),
    (r"Software\Microsoft\Edge", false),
    (r"Software\BraveSoftware\Brave-Browser", false),
    (r"Software\Chromium", false),
    (r"Software\Vivaldi", false),
    (r"Software\Mozilla", true),
];

/// Write both manifests to the app data folder and point every browser's key at them.
/// Keys for browsers that are not installed are harmless (nothing reads them).
#[cfg(windows)]
pub fn register(exe: &Path, data_dir: &Path) -> Vec<PathBuf> {
    let dir = data_dir.join("native-messaging");
    let chrome = dir.join(format!("{HOST_NAME}.chrome.json"));
    let firefox = dir.join(format!("{HOST_NAME}.firefox.json"));
    let mut written = Vec::new();
    for (path, manifest) in [(&chrome, chrome_manifest(exe)), (&firefox, firefox_manifest(exe))] {
        match write_if_changed(path, &manifest) {
            Ok(()) => written.push(path.clone()),
            Err(e) => log::warn!("host manifest {}: {e}", path.display()),
        }
    }
    for (browser, is_firefox) in WINDOWS_BROWSERS {
        let key = format!(r"{browser}\NativeMessagingHosts\{HOST_NAME}");
        let target = if *is_firefox { &firefox } else { &chrome };
        let result = windows_registry::CURRENT_USER
            .create(&key)
            .and_then(|k| k.set_string("", target.to_string_lossy().as_ref()));
        if let Err(e) = result {
            log::warn!("host registry key {key}: {e}");
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_allow_only_the_warden_extension() {
        let exe = Path::new("/Applications/Warden β.app/Contents/MacOS/warden-native-host");
        let chrome = chrome_manifest(exe);
        assert_eq!(chrome["allowed_origins"], json!(["chrome-extension://blcmgpbfbbemkkcfjgbhgblnapbndfgp/"]));
        assert_eq!(chrome["path"], "/Applications/Warden β.app/Contents/MacOS/warden-native-host");
        assert_eq!(chrome["type"], "stdio");
        assert!(chrome.get("allowed_extensions").is_none());
        let firefox = firefox_manifest(exe);
        assert_eq!(firefox["allowed_extensions"], json!(["extension-staging@getwarden.app"]));
        assert!(firefox.get("allowed_origins").is_none());
    }

    #[test]
    fn host_name_follows_the_browsers_rules() {
        // Chrome: lowercase letters, digits, underscores and dots; no dot at either end, no "..".
        let ok = HOST_NAME.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
            && !HOST_NAME.starts_with('.')
            && !HOST_NAME.ends_with('.')
            && !HOST_NAME.contains("..");
        assert!(ok);
    }

    #[test]
    fn writing_twice_leaves_the_file_as_is() {
        let dir = std::env::temp_dir().join(format!("warden-manifest-{}", uuid::Uuid::new_v4()));
        let path = dir.join("NativeMessagingHosts").join(file_name());
        let manifest = chrome_manifest(Path::new("/x/warden"));
        write_if_changed(&path, &manifest).unwrap();
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        write_if_changed(&path, &manifest).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
        let moved = chrome_manifest(Path::new("/y/warden"));
        write_if_changed(&path, &moved).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("/y/warden"), "a moved app rewrites the path");
        std::fs::remove_dir_all(dir).ok();
    }
}
