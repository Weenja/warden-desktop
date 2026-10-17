//! Windows placement. STUB from the macOS builder so the crate compiles: plain Tauri
//! monitor math. The Windows builder replaces it (top-centre pill, work-area shield, taskbar
//! flyout).

use tauri::{Rect, WebviewWindow};

pub fn pill(win: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    super::generic::pill(win, width, height)
}

pub fn shield(win: &WebviewWindow) -> tauri::Result<()> {
    super::generic::shield(win)
}

pub fn popover(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
    super::popover_near_icon(win, icon)
}
