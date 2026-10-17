//! Creating and placing the app's windows. (Named `ui_windows` so it never clashes with the
//! `windows` crate the Windows build uses.)
//!
//! - `signin`: a normal window for sign-in and "This Mac is linked". Only shown when needed.
//! - `popover`: a borderless panel under the menu bar / tray icon. Created hidden at start so
//!   it opens instantly; hides when it loses focus, like a system menu.
//! - `pill`, `shield`: created hidden at start, shown by the drain loop. Placed (and made to
//!   float) by [`crate::placement`].
//! - `toast`: the one-time low battery note; closes itself after a few seconds.
//! - `picker`, `quit`: normal windows opened from the popover.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::guard::Update;
use crate::placement;

pub const SIGNIN: &str = "signin";
pub const POPOVER: &str = "popover";
pub const PILL: &str = "pill";
pub const SHIELD: &str = "shield";
pub const TOAST: &str = "toast";
pub const PICKER: &str = "picker";
pub const QUIT: &str = "quit";

const SIGNIN_WIDTH: f64 = 380.0;
const POPOVER_WIDTH: f64 = 330.0;
const PICKER_WIDTH: f64 = 420.0;
const QUIT_WIDTH: f64 = 360.0;
const TOAST_WIDTH: f64 = 330.0;
/// How long the low battery toast stays.
const TOAST_FOR: Duration = Duration::from_secs(8);

/// The pill's last size from its page, so a re-show (maybe on another screen) keeps it.
static PILL_SIZE: Mutex<(f64, f64)> = Mutex::new((200.0, 32.0));

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
    placement::popover(&win, icon)?;
    win.show()?;
    win.set_focus()
}

/// A borderless, transparent window that floats and never shows in the taskbar.
fn floating(app: &AppHandle, label: &str, page: &str, width: f64, height: f64) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title("Warden")
        .inner_size(width, height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        // The first click on the pill / shield acts at once instead of only focusing it.
        .accept_first_mouse(true)
        .visible(false)
        .build()
}

/// The pill and the shield exist from the start (hidden), so they appear without a delay.
/// They are shown only through `placement` (see there why).
pub fn create_drain_windows(app: &AppHandle) -> tauri::Result<()> {
    let (w, h) = *PILL_SIZE.lock().unwrap();
    floating(app, PILL, "pill.html", w, h)?;
    floating(app, SHIELD, "shield.html", 800.0, 600.0)?;
    Ok(())
}

/// The pill page measured itself: keep the size, and resize and re-pin it if it is showing.
pub fn fit_pill(app: &AppHandle, width: f64, height: f64) -> tauri::Result<()> {
    *PILL_SIZE.lock().unwrap() = (width, height);
    match app.get_webview_window(PILL) {
        Some(win) if win.is_visible()? => placement::pill(&win, width, height),
        _ => Ok(()),
    }
}

/// Show or hide the pill, shield and toast after the drain loop changed its view.
pub fn apply_guard_update(app: &AppHandle, update: &Update) {
    if let Some(pill) = app.get_webview_window(PILL) {
        let result = match &update.view.drain {
            Some(_) if !pill.is_visible().unwrap_or(false) => {
                let (w, h) = *PILL_SIZE.lock().unwrap();
                placement::pill(&pill, w, h)
            }
            Some(_) => Ok(()),
            None => pill.hide(),
        };
        if let Err(e) = result {
            log::warn!("pill: {e}");
        }
    }
    if let Some(shield) = app.get_webview_window(SHIELD) {
        let result = match &update.view.shield {
            // Placed on every show: the locked app may be on another screen than last time.
            Some(_) if !shield.is_visible().unwrap_or(false) => placement::shield(&shield),
            Some(_) => Ok(()),
            None => shield.hide(),
        };
        if let Err(e) = result {
            log::warn!("shield: {e}");
        }
    }
    if update.low_toast {
        if let Err(e) = open_toast(app) {
            log::warn!("toast: {e}");
        }
    }
}

fn open_toast(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(TOAST).is_some() {
        return Ok(());
    }
    let win = floating(app, TOAST, "toast.html", TOAST_WIDTH, 76.0)?;
    placement::toast(&win)?;
    let handle = win.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TOAST_FOR).await;
        let _ = handle.close();
    });
    Ok(())
}

/// A normal window in front of everything else: the picker and the quit confirm.
fn open_dialog(app: &AppHandle, label: &str, page: &str, width: f64) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(label) {
        win.show()?;
        return win.set_focus();
    }
    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title("Warden")
        .inner_size(width, 420.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .center()
        .focused(true)
        .build()?;
    win.set_focus()
}

/// "Edit" in the popover: the guarded apps picker.
pub fn open_picker(app: &AppHandle) -> tauri::Result<()> {
    open_dialog(app, PICKER, "picker.html", PICKER_WIDTH)
}

/// "Quit Warden" in the popover asks first: the crew gets told.
pub fn open_quit(app: &AppHandle) -> tauri::Result<()> {
    open_dialog(app, QUIT, "quit.html", QUIT_WIDTH)
}
