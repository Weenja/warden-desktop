//! macOS placement. Stand-in on the `windows` branch so the shared interface compiles; the
//! real macOS file (notch pill, shield) comes from main and replaces this one at merge.

use tauri::{PhysicalPosition, Rect, WebviewWindow};

pub fn pill(win: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    let Some(monitor) = win.primary_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let x = monitor.position().x as f64 + (monitor.size().width as f64 - width * scale) / 2.0;
    win.set_size(tauri::LogicalSize::new(width, height))?;
    win.set_position(PhysicalPosition::new(x.round() as i32, monitor.position().y))
}

pub fn shield(win: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = win.current_monitor()? else {
        return Ok(());
    };
    let work = monitor.work_area();
    win.set_position(work.position)?;
    win.set_size(work.size)
}

pub fn popover(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
    crate::ui_windows::place_popover(win, icon)
}
