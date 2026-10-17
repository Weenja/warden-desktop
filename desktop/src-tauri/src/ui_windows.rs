//! Creating and placing the app's windows. (Named `ui_windows` so it never clashes with the
//! `windows` crate the Windows build uses.)
//!
//! - `signin`: a normal window for sign-in and "This Mac is linked". Only shown when needed.
//! - `popover`: a borderless panel under the menu bar / tray icon. Created hidden at start so
//!   it opens instantly; hides when it loses focus, like a system menu.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub const SIGNIN: &str = "signin";
pub const POPOVER: &str = "popover";

const SIGNIN_WIDTH: f64 = 380.0;
const POPOVER_WIDTH: f64 = 330.0;
/// Gap between the menu bar and the popover, in logical pixels.
#[cfg(target_os = "macos")]
const POPOVER_GAP: f64 = 6.0;

/// When the popover last hid because it lost focus. A click on the tray icon first steals
/// focus (hiding the popover) and then arrives as a click; without this the click would
/// immediately show it again instead of closing it.
static POPOVER_HIDDEN_AT_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Show the sign-in window, creating it if needed.
pub fn open_signin(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(SIGNIN) {
        win.show()?;
        return win.set_focus();
    }
    let win = WebviewWindowBuilder::new(app, SIGNIN, WebviewUrl::App("signin.html".into()))
        .title("Warden")
        .inner_size(SIGNIN_WIDTH, 480.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .focused(true)
        .build()?;
    win.set_focus()
}

pub fn close_signin(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(SIGNIN) {
        let _ = win.close();
    }
}

pub fn create_popover(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let win = WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("popover.html".into()))
        .title("Warden")
        .inner_size(POPOVER_WIDTH, 520.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    let handle = win.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            POPOVER_HIDDEN_AT_MS.store(now_ms(), Ordering::Relaxed);
            let _ = handle.hide();
        }
    });
    Ok(win)
}

/// Tray icon clicked: open the popover under it, or close it if it is open.
pub fn toggle_popover(app: &AppHandle, icon: Rect) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window(POPOVER) else {
        return Ok(());
    };
    let just_hidden = now_ms().saturating_sub(POPOVER_HIDDEN_AT_MS.load(Ordering::Relaxed)) < 300;
    if win.is_visible()? || just_hidden {
        return win.hide();
    }
    crate::placement::popover(&win, icon)?;
    win.show()?;
    win.set_focus()
}

/// Centre the popover under the menu bar icon, kept inside the screen. (Windows uses the tray
/// flyout placement in `placement/windows.rs`.)
#[cfg(target_os = "macos")]
pub(crate) fn place_popover(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
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
    win.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32))
}
