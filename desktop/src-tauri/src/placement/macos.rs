//! macOS placement. The pill, shield and toast are NSPanels (tauri-nspanel turns the Tauri
//! window into one) so they can:
//! - float above full-screen apps and show on every Space (collection behaviour);
//! - take clicks without activating Warden ("non-activating"), so the guarded app stays the
//!   frontmost app and the drain loop keeps seeing it;
//! - sit above the menu bar (the pill merges with the notch).
//!
//! Permissions: none. Reading window positions with CGWindowListCopyWindowInfo needs no
//! Screen Recording permission; Warden reads only owner, layer and bounds, never titles.

use std::sync::mpsc;

use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSScreen, NSWorkspace};
use objc2_core_foundation::CFString;
use objc2_core_graphics::{
    kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID, CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString};
use tauri::{Emitter, Manager, Rect, WebviewWindow};
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask, WebviewWindowExt};

use super::PillMetrics;
use panels::{track_hover, PillPanel, ShieldPanel, ToastPanel};

/// The panel classes. In their own module because the macro brings AppKit names into scope.
mod panels {
    use tauri_nspanel::{tauri_panel, TrackingAreaOptions};

    tauri_panel! {
    // Hover is tracked by the panel even while Warden is not the active app: web views only
    // see the mouse in the active app.
    panel!(PillPanel {
        config: { can_become_key_window: false, is_floating_panel: true }
        with: {
            tracking_area: {
                options: TrackingAreaOptions::new().active_always().mouse_entered_and_exited(),
                auto_resize: true
            }
        }
    })
    // The shield takes the keyboard (key window) without activating Warden, so the locked app
    // gets no key presses while it stays the frontmost app.
    panel!(ShieldPanel {
        config: { can_become_key_window: true, is_floating_panel: true }
    })
    panel!(ToastPanel {
        config: { can_become_key_window: false, is_floating_panel: true }
    })
    panel_event!(PillHover {})
    }

    /// Call `enter` / `exit` when the mouse enters or leaves the panel.
    pub fn track_hover(
        panel: &tauri_nspanel::PanelHandle<tauri::Wry>,
        enter: impl Fn() + 'static,
        exit: impl Fn() + 'static,
    ) {
        let handler = PillHover::new();
        handler.on_mouse_entered(move |_| enter());
        handler.on_mouse_exited(move |_| exit());
        // The panel keeps the handler alive.
        panel.set_event_handler(Some(handler.as_ref()));
    }
}

/// Above the menu bar (24) and its status items (25), below open menus (101).
const PILL_LEVEL: i32 = 27;
/// The event the pill page listens to for hover.
pub const PILL_HOVER_EVENT: &str = "pill-hover";
/// Fallback menu bar height when the screen does not report one (menu bar hidden).
const DEFAULT_BAR_HEIGHT: f64 = 24.0;
/// Inset of the toast from the screen corner.
const TOAST_MARGIN: f64 = 12.0;

/// Floats on every Space, also over full-screen apps; not part of Cmd-` window cycling.
fn floating_everywhere() -> CollectionBehavior {
    CollectionBehavior::new().can_join_all_spaces().stationary().full_screen_auxiliary().ignores_cycle()
}

pub fn pill(win: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    let win = win.clone();
    on_main(&win.clone(), move |mtm| {
        let panel = panel_for(&win, PanelKind::Pill)?;
        let screen = screen_in_use(mtm);
        let frame = screen.frame();
        // Top edge on the top of the screen, centred on the notch: it grows out of it.
        let origin = NSPoint::new(notch_centre_x(&screen) - width / 2.0, frame.origin.y + frame.size.height - height);
        panel.as_panel().setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
        // In front without becoming key: the guarded app keeps the keyboard and stays frontmost.
        panel.order_front_regardless();
        Ok(())
    })?
}

pub fn shield(win: &WebviewWindow) -> tauri::Result<()> {
    let win = win.clone();
    on_main(&win.clone(), move |mtm| {
        let panel = panel_for(&win, PanelKind::Shield)?;
        let screen = screen_in_use(mtm);
        let frame = screen.frame();
        let below_bar = NSSize::new(frame.size.width, frame.size.height - metrics(&screen).bar_height);
        panel.as_panel().setFrame_display(NSRect::new(frame.origin, below_bar), true);
        // Key window (keyboard) without activating Warden: the locked app stays frontmost.
        panel.show_and_make_key();
        Ok(())
    })?
}

pub fn toast(win: &WebviewWindow) -> tauri::Result<()> {
    let win = win.clone();
    on_main(&win.clone(), move |mtm| {
        let panel = panel_for(&win, PanelKind::Toast)?;
        let screen = screen_in_use(mtm);
        let frame = screen.frame();
        let size = panel.as_panel().frame().size;
        let origin = NSPoint::new(
            frame.origin.x + frame.size.width - size.width - TOAST_MARGIN,
            frame.origin.y + frame.size.height - metrics(&screen).bar_height - TOAST_MARGIN - size.height,
        );
        panel.as_panel().setFrameOrigin(origin);
        panel.order_front_regardless();
        Ok(())
    })?
}

pub fn popover(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
    super::popover_near_icon(win, icon)
}

pub fn pill_metrics(win: &WebviewWindow) -> PillMetrics {
    on_main(win, |mtm| metrics(&screen_in_use(mtm)))
        .unwrap_or(PillMetrics { notch_width: 0.0, bar_height: DEFAULT_BAR_HEIGHT })
}

#[derive(Clone, Copy)]
enum PanelKind {
    Pill,
    Shield,
    Toast,
}

/// The window as an NSPanel, converted and set up on first use.
fn panel_for(win: &WebviewWindow, kind: PanelKind) -> tauri::Result<PanelHandle<tauri::Wry>> {
    let app = win.app_handle();
    if let Ok(panel) = app.get_webview_panel(win.label()) {
        return Ok(panel);
    }
    let (panel, level) = match kind {
        PanelKind::Pill => (win.to_panel::<PillPanel>()?, PanelLevel::Custom(PILL_LEVEL)),
        // Above the app's own floating palettes, below the Dock and the menu bar, so the user
        // can still switch apps and see the pill.
        PanelKind::Shield => (win.to_panel::<ShieldPanel>()?, PanelLevel::ModalPanel),
        PanelKind::Toast => (win.to_panel::<ToastPanel>()?, PanelLevel::Status),
    };
    panel.set_level(level.value());
    panel
        .set_style_mask(StyleMask::empty().borderless().nonactivating_panel().into())
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    panel.set_collection_behavior(floating_everywhere().into());
    panel.set_hides_on_deactivate(false);
    panel.set_has_shadow(false);
    if let PanelKind::Pill = kind {
        let (enter, exit) = (app.clone(), app.clone());
        let (label, label_exit) = (win.label().to_string(), win.label().to_string());
        track_hover(
            &panel,
            move || {
                let _ = enter.emit_to(label.as_str(), PILL_HOVER_EVENT, true);
            },
            move || {
                let _ = exit.emit_to(label_exit.as_str(), PILL_HOVER_EVENT, false);
            },
        );
    }
    Ok(panel)
}

/// Run `f` on the main thread (AppKit's rule) and return its result.
fn on_main<T: Send + 'static>(
    win: &WebviewWindow,
    f: impl FnOnce(MainThreadMarker) -> T + Send + 'static,
) -> tauri::Result<T> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(mtm));
    }
    let (tx, rx) = mpsc::channel();
    win.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
        let _ = tx.send(f(mtm));
    })?;
    rx.recv().map_err(|_| tauri::Error::FailedToReceiveMessage)
}

fn metrics(screen: &NSScreen) -> PillMetrics {
    PillMetrics { notch_width: notch(screen).map_or(0.0, |(_, w)| w), bar_height: menu_bar_height(screen) }
}

/// The notch as (left edge x, width) in global coordinates, from the menu bar areas left and
/// right of the camera housing. None on screens without one.
fn notch(screen: &NSScreen) -> Option<(f64, f64)> {
    if screen.safeAreaInsets().top <= 0.0 {
        return None;
    }
    let (left, right) = (screen.auxiliaryTopLeftArea(), screen.auxiliaryTopRightArea());
    let width = right.origin.x - (left.origin.x + left.size.width);
    (left.size.width > 0.0 && width > 0.0).then(|| (screen.frame().origin.x + left.size.width, width))
}

/// The x the pill centres on: the notch, or the middle of the menu bar.
fn notch_centre_x(screen: &NSScreen) -> f64 {
    match notch(screen) {
        Some((x, width)) => x + width / 2.0,
        None => screen.frame().origin.x + screen.frame().size.width / 2.0,
    }
}

fn menu_bar_height(screen: &NSScreen) -> f64 {
    let inset = screen.safeAreaInsets().top;
    if inset > 0.0 {
        return inset;
    }
    let (frame, visible) = (screen.frame(), screen.visibleFrame());
    let bar = (frame.origin.y + frame.size.height) - (visible.origin.y + visible.size.height);
    if bar > 0.0 {
        bar
    } else {
        DEFAULT_BAR_HEIGHT
    }
}

/// The screen the user works on: the one with the frontmost app's front window (so the pill and
/// shield follow a guarded app to a second display), else the main screen.
fn screen_in_use(mtm: MainThreadMarker) -> objc2::rc::Retained<NSScreen> {
    let screens = NSScreen::screens(mtm);
    let by_window = front_window_centre(&screens).and_then(|point| {
        screens.iter().find(|s| {
            let f = s.frame();
            point.x >= f.origin.x
                && point.x < f.origin.x + f.size.width
                && point.y >= f.origin.y
                && point.y < f.origin.y + f.size.height
        })
    });
    by_window
        .or_else(|| NSScreen::mainScreen(mtm))
        .or_else(|| screens.firstObject())
        .expect("a Mac has at least one screen")
}

/// Centre of the frontmost app's frontmost normal window, in AppKit coordinates.
fn front_window_centre(screens: &NSArray<NSScreen>) -> Option<NSPoint> {
    let pid = NSWorkspace::sharedWorkspace().frontmostApplication()?.processIdentifier();
    let list = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    // CFArray of CFDictionary is toll-free bridged to NSArray of NSDictionary.
    let windows: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe {
        &*(&*list as *const objc2_core_foundation::CFArray as *const NSArray<NSDictionary<NSString, AnyObject>>)
    };
    let number = |dict: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        dict.objectForKey(key).and_then(|v| v.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
    };
    let (owner, layer, bounds) =
        unsafe { (cf_key(kCGWindowOwnerPID), cf_key(kCGWindowLayer), cf_key(kCGWindowBounds)) };
    // The list is ordered front to back: the first normal-layer window of the app is its front one.
    let window = windows.iter().find(|w| number(w, owner) == Some(f64::from(pid)) && number(w, layer) == Some(0.0))?;
    let rect = window.objectForKey(bounds)?.downcast::<NSDictionary>().ok()?;
    let rect: &NSDictionary<NSString, AnyObject> =
        unsafe { &*(&*rect as *const NSDictionary as *const NSDictionary<NSString, AnyObject>) };
    let get = |k: &str| number(rect, &NSString::from_str(k));
    let (x, y, w, h) = (get("X")?, get("Y")?, get("Width")?, get("Height")?);
    // CoreGraphics measures from the top of the primary screen, AppKit from its bottom.
    let primary_height = screens.firstObject()?.frame().size.height;
    Some(NSPoint::new(x + w / 2.0, primary_height - (y + h / 2.0)))
}

/// A CFString key as NSString (toll-free bridged).
///
/// # Safety
/// `key` must be a valid CFString constant.
unsafe fn cf_key(key: &CFString) -> &NSString {
    unsafe { &*(key as *const CFString as *const NSString) }
}
