//! What Warden needs from the operating system, behind one small trait so the drain loop and
//! the UI never touch OS APIs directly. One implementation per OS:
//! `macos.rs` (AppKit) and `windows.rs` (Win32).

use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

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

/// Browsers Warden knows (macOS bundle ids and Windows exe names). While the user has guarded
/// sites, a browser without the Warden extension drains the whole time it is in front, because
/// Warden cannot see which site is open in it.
pub const KNOWN_BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.google.Chrome",
    "org.mozilla.firefox",
    "company.thebrowser.Browser",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
];
