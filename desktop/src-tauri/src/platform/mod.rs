//! What Warden needs from the operating system, behind one small trait so the drain loop and
//! the UI never touch OS APIs directly. One implementation per OS:
//! `macos.rs` (AppKit) and `windows.rs` (Win32).

use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// Windows helpers without Win32: compiled for Windows, and for tests on every OS.
#[cfg(any(target_os = "windows", test))]
mod exe_names;
#[cfg(any(target_os = "windows", test))]
mod icon_pixels;

/// The app in front. `id` is the stable key stored in the block list: the bundle id on macOS
/// (`com.valvesoftware.steam`), the exe file name on Windows (`steam.exe`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrontApp {
    pub id: String,
    pub name: String,
}

/// An installed app for the picker. `icon_png` is a small PNG (64 px) when the OS has one.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub icon_png: Option<Vec<u8>>,
}

pub trait Platform: Send + Sync {
    /// The app that has keyboard focus right now, or None (e.g. at the login window).
    fn frontmost_app(&self) -> Option<FrontApp>;

    /// Ask every running instance of `id` to quit (like Cmd-Q / closing the window).
    /// Returns true when at least one instance was asked.
    fn terminate_app(&self, id: &str) -> bool;

    /// Apps the user can pick to guard. Slow (reads the disk); call off the UI thread.
    fn installed_apps(&self) -> Vec<InstalledApp>;

    /// True while the screen is locked. Nothing drains then, and unlocking sends a heartbeat.
    fn screen_locked(&self) -> bool;

    /// The computer's name as the user set it ("Sam's MacBook Air"), shown to the crew.
    fn device_name(&self) -> String;
}

/// The process that runs `pid`'s parent, its app id (bundle id on macOS, exe name on Windows)
/// and its exe file. Used only to check who is on the other end of the browser link.
#[cfg(target_os = "macos")]
use macos::{process_app_id, process_exe, process_parent};
#[cfg(target_os = "windows")]
use windows::{process_app_id, process_exe, process_parent};

/// The exe file `name` (without `.exe`) in the folder of this process's own exe. Warden ships
/// two programs side by side: the app (`warden`) and its native messaging helper
/// (`warden-native-host`); Tauri bundles both into `Contents/MacOS/` of the .app and into the
/// install folder on Windows.
pub fn sibling_exe(name: &str) -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
}

/// True when process `pid` runs exactly the exe file at `path`. Both paths are resolved first,
/// so a link or a `..` in a path cannot make a different file look like Warden's.
pub fn runs_exe(pid: u32, path: &std::path::Path) -> bool {
    match (process_exe(pid).map(|p| p.canonicalize()), path.canonicalize()) {
        (Some(Ok(peer)), Ok(expected)) => peer == expected,
        _ => false,
    }
}

/// The known browser that started process `pid`, walking up its parents (Chrome on Windows
/// starts hosts through cmd.exe). None when no known browser is above it, e.g. the host was
/// started by hand from a terminal; such a link never changes what drains.
pub fn browser_ancestor(pid: u32) -> Option<String> {
    let mut current = pid;
    // A few levels are enough (browser -> cmd.exe -> host); the cap also stops parent-id loops.
    for _ in 0..6 {
        current = process_parent(current).filter(|p| *p > 1 && *p != current)?;
        if let Some(id) = process_app_id(current).filter(|id| KNOWN_BROWSERS.contains(&id.as_str())) {
            return Some(id);
        }
    }
    None
}

/// The implementation for the OS this binary was built for.
pub fn current() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    return Box::new(macos::MacPlatform);
    #[cfg(target_os = "windows")]
    return Box::new(windows::WindowsPlatform);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    compile_error!("Warden desktop supports macOS and Windows only");
}

/// Which `warden.platform` value this build registers as.
pub fn device_platform() -> crate::api::DevicePlatform {
    #[cfg(target_os = "windows")]
    return crate::api::DevicePlatform::Windows;
    #[cfg(not(target_os = "windows"))]
    return crate::api::DevicePlatform::Macos;
}

/// Browsers Warden knows on this OS. While the user has guarded sites, a browser without the
/// Warden extension drains the whole time it is in front, because Warden cannot see which
/// site is open in it.
#[cfg(target_os = "macos")]
pub const KNOWN_BROWSERS: &[&str] = MACOS_BROWSERS;
#[cfg(target_os = "windows")]
pub const KNOWN_BROWSERS: &[&str] = WINDOWS_BROWSERS;

/// macOS bundle ids.
pub const MACOS_BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.google.Chrome",
    "org.mozilla.firefox",
    "company.thebrowser.Browser",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
];

/// Windows exe names, lowercase like every Windows app id. Opera GX runs as opera.exe.
pub const WINDOWS_BROWSERS: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "arc.exe",
    "chromium.exe",
    "librewolf.exe",
    "waterfox.exe",
    "floorp.exe",
    "zen.exe",
    "thorium.exe",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_browsers_are_written_as_app_ids() {
        // A capital letter or a missing .exe would never match the app in front.
        for browser in WINDOWS_BROWSERS {
            assert_eq!(exe_names::exe_id(browser).as_deref(), Some(*browser));
        }
    }

    #[test]
    fn runs_exe_matches_only_the_exact_file() {
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        assert!(runs_exe(me, &exe));
        // The same folder, another name (what the app expects from the helper): refused.
        assert!(!runs_exe(me, &sibling_exe("warden-native-host").unwrap()));
        // A path with `..` that leads back to the same file still matches.
        let dir = exe.parent().unwrap();
        let roundabout = dir.join("..").join(dir.file_name().unwrap()).join(exe.file_name().unwrap());
        assert!(runs_exe(me, &roundabout));
    }
}
