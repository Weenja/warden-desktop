//! Where Warden's own windows sit on screen, and how they float. The only module with
//! OS-specific window code: `macos.rs` (AppKit panels at the notch) and `windows.rs` (Win32).
//! Everything else creates windows with plain Tauri builders and places them through here.
//!
//! - `pill`: the drain pill, at the notch (Mac) or the top centre (Windows). Never takes focus.
//! - `shield`: covers the screen under the menu bar while a guarded app is locked.
//! - `popover`: the panel under the menu bar / tray icon.
//! - `toast`: the one-time low battery note, top right.

use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::PhysicalPosition;
use tauri::{Rect, WebviewWindow};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as os;

/// Pure taskbar/flyout geometry, used by `windows.rs` and unit-tested on every OS.
#[cfg(any(windows, test))]
mod taskbar;

/// Size the pill window to `width` x `height` (logical px), pin it to the top of the screen and
/// show it without taking focus. Callers never call `show()` on the pill, shield or toast: a
/// plain show takes focus (and on Windows resets the tool-window style).
pub fn pill(win: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    os::pill(win, width, height)
}

/// Cover the screen below the menu bar, above normal windows, on every Space, and show it.
pub fn shield(win: &WebviewWindow) -> tauri::Result<()> {
    os::shield(win)
}

/// Put the popover next to the menu bar / tray icon at `icon`.
pub fn popover(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
    os::popover(win, icon)
}

/// Put the low battery toast in the top right corner of the screen in use, and show it.
pub fn toast(win: &WebviewWindow) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    return macos::toast(win);
    #[cfg(not(target_os = "macos"))]
    return generic::toast(win);
}

/// What the pill page needs to line up with the screen: the notch width (0 = no notch) and the
/// menu bar height, in logical px.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PillMetrics {
    pub notch_width: f64,
    pub bar_height: f64,
}

pub fn pill_metrics(win: &WebviewWindow) -> PillMetrics {
    #[cfg(target_os = "macos")]
    return macos::pill_metrics(win);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = win;
        PillMetrics { notch_width: 0.0, bar_height: 30.0 }
    }
}

/// Gap between the menu bar and the popover, in logical pixels.
#[cfg(target_os = "macos")]
const POPOVER_GAP: f64 = 6.0;

/// Centre the popover on the menu bar icon (below it; above it if the icon is ever at the
/// bottom), kept inside the screen. Windows uses the tray flyout math in `taskbar.rs`.
#[cfg(target_os = "macos")]
fn popover_near_icon(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
    let scale = win.scale_factor()?;
    let pos = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let win_size = win.outer_size()?;
    let (w, h) = (win_size.width as f64, win_size.height as f64);
    let gap = POPOVER_GAP * scale;

    let monitor = win.monitor_from_point(pos.x, pos.y)?.or(win.current_monitor()?);
    let (mx, my, mw, mh) = match &monitor {
        Some(m) => (m.position().x as f64, m.position().y as f64, m.size().width as f64, m.size().height as f64),
        None => (0.0, 0.0, f64::MAX, f64::MAX),
    };

    let icon_at_top = pos.y - my < mh / 2.0;
    let y = if icon_at_top { pos.y + size.height + gap } else { pos.y - h - gap };
    let x = (pos.x + size.width / 2.0 - w / 2.0).clamp(mx + gap, (mx + mw - w - gap).max(mx + gap));
    win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

/// Placement with Tauri's own monitor API, for windows that need no OS-specific code (the
/// low battery toast on Windows).
#[cfg(not(target_os = "macos"))]
mod generic {
    use tauri::{LogicalPosition, WebviewWindow};

    /// Inset of the toast from the screen corner, in logical px.
    const TOAST_MARGIN: f64 = 12.0;

    /// The monitor's position and size in logical px.
    fn monitor(win: &WebviewWindow) -> tauri::Result<(f64, f64, f64, f64)> {
        let Some(m) = win.current_monitor()?.or(win.primary_monitor()?) else {
            return Ok((0.0, 0.0, 1280.0, 800.0));
        };
        let pos = m.position().to_logical::<f64>(m.scale_factor());
        let size = m.size().to_logical::<f64>(m.scale_factor());
        Ok((pos.x, pos.y, size.width, size.height))
    }

    pub fn toast(win: &WebviewWindow) -> tauri::Result<()> {
        let (x, y, w, _) = monitor(win)?;
        let size = win.outer_size()?.to_logical::<f64>(win.scale_factor()?);
        win.set_position(LogicalPosition::new(x + w - size.width - TOAST_MARGIN, y + TOAST_MARGIN))?;
        win.show()
    }
}
