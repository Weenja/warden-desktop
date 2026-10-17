//! The menu bar (macOS) / tray (Windows) icon. A click opens the popover.

use tauri::image::Image;
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::ui_windows;

/// macOS: MOA's black silhouette with alpha, 44 px (22 pt @2x). As a template image macOS
/// tints it to match the menu bar (dark, light, highlighted).
#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");
/// Windows: the colour mochi, 32 px (the tray draws 16 px at 100 % and 32 px at 200 %).
/// Windows does not tint tray icons, so a colour mark reads on both light and dark taskbars.
#[cfg(target_os = "windows")]
const ICON: &[u8] = include_bytes!("../icons/tray-color.png");

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("warden")
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
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
