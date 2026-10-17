//! The menu bar (macOS) / tray (Windows) icon: MOA's silhouette. A click opens the popover.

use tauri::image::Image;
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::ui_windows;

/// Black silhouette with alpha, 44 px (22 pt @2x). As a template image macOS tints it to
/// match the menu bar (dark, light, highlighted).
const ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("warden")
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .tooltip("Warden")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { rect, button_state: MouseButtonState::Up, .. } = event {
                if let Err(e) = ui_windows::toggle_popover(tray.app_handle(), rect) {
                    log::warn!("popover: {e}");
                }
            }
        })
        .build(app)?;
    Ok(())
}
