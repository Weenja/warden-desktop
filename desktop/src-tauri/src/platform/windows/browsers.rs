//! Which exes are web browsers, so a browser Warden has never heard of still drains (see
//! "Browsers" in `platform/mod.rs`). Three checks, any one is enough:
//! - Windows' own list of installed browsers, the one Settings > Default apps shows
//!   (`Clients\StartMenuInternet` in the registry). Every browser installer writes it.
//! - A fixed list of browser exe names, for portable copies that were never installed.
//! - The exe name the browser's maker built into the file (`OriginalFilename`), so a portable
//!   copy renamed to hide it (`chrome.exe` -> `notes.exe`) still counts.
//!
//! Read only; nothing is changed.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE};

use super::version_string;
use crate::platform::exe_names::{command_exe, exe_id};
use crate::platform::WINDOWS_BROWSERS;

/// Per-user installs, then all-users installs (64-bit and 32-bit views).
const REGISTERED_BROWSER_KEYS: [(&Key, &str); 3] = [
    (CURRENT_USER, r"Software\Clients\StartMenuInternet"),
    (LOCAL_MACHINE, r"SOFTWARE\Clients\StartMenuInternet"),
    (LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Clients\StartMenuInternet"),
];

/// An exe file as it was checked: its path and the time it was last changed.
type ExeVersion = (String, Option<SystemTime>);

/// True when the exe at `path` is a web browser. Cached per (path, file time): the drain loop
/// asks every second, and a different file copied over the same path is checked again.
pub fn is_browser_exe(path: &str) -> bool {
    static CHECKED: OnceLock<Mutex<HashMap<ExeVersion, bool>>> = OnceLock::new();
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let key = (path.to_string(), modified);
    let checked = CHECKED.get_or_init(Default::default);
    if let Some(known) = checked.lock().unwrap().get(&key) {
        return *known;
    }
    let built_in_name = version_string(path, "OriginalFilename").and_then(|name| exe_id(&name));
    let browser = exe_id(path).is_some_and(|id| browser_ids().contains(&id))
        || built_in_name.is_some_and(|name| WINDOWS_BROWSERS.contains(&name.as_str()));
    checked.lock().unwrap().insert(key, browser);
    browser
}

/// Exe ids of every browser: the fixed list plus the ones registered with Windows.
pub fn browser_ids() -> Vec<String> {
    let mut ids: Vec<String> = WINDOWS_BROWSERS.iter().map(|id| id.to_string()).collect();
    ids.extend(registered_browsers());
    ids
}

/// Exe ids of the browsers registered with Windows. Each entry's `shell\open\command` is the
/// command that starts it, e.g. `"C:\Program Files\Zen Browser\zen.exe"`.
fn registered_browsers() -> Vec<String> {
    let mut found = Vec::new();
    for (root, path) in REGISTERED_BROWSER_KEYS {
        let Ok(list) = root.open(path) else { continue };
        let Ok(names) = list.keys() else { continue };
        for name in names {
            let command = list.open(format!(r"{name}\shell\open\command")).and_then(|key| key.get_hstring(""));
            if let Some(id) = command.ok().and_then(|c| command_exe(&c.to_string_lossy())).and_then(|exe| exe_id(&exe))
            {
                found.push(id);
            }
        }
    }
    found
}
