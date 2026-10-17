//! Where Warden's floating windows go: the drain pill, the shield and the tray popover.
//! One file per OS, because the menu bar and the taskbar work differently.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

// Pure taskbar maths: compiled for Windows, and for tests on every OS.
#[cfg(any(target_os = "windows", test))]
mod taskbar;

/// Place the drain pill (`width` x `height` logical px) at the top centre of the screen,
/// always on top, without a taskbar button and without taking focus.
pub fn pill(win: &tauri::WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    os::pill(win, width, height)
}

/// Cover the screen's work area with the shield (the menu bar / taskbar stays visible).
pub fn shield(win: &tauri::WebviewWindow) -> tauri::Result<()> {
    os::shield(win)
}

/// Put the popover next to the tray / menu bar icon at `icon`.
pub fn popover(win: &tauri::WebviewWindow, icon: tauri::Rect) -> tauri::Result<()> {
    os::popover(win, icon)
}
