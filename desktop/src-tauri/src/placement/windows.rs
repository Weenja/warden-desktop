//! Window placement on Windows: the pill at the top centre, the shield over the work area (the
//! taskbar stays visible), the popover as a tray flyout.
//!
//! Tauri's window layer (tao) rewrites a window's extended styles from its own flags whenever
//! one of them changes. So everything tao knows (topmost, no taskbar button, not focusable) is
//! set through Tauri, and only what it does not know (WS_EX_TOOLWINDOW, which keeps the pill
//! out of Alt+Tab) is added with Win32 afterwards, on the main thread so it lands after tao.

use tauri::{PhysicalPosition, PhysicalSize, Rect, WebviewWindow};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

use super::taskbar::{flyout_origin, Area};

/// Gap between the top of the screen and the pill, in logical pixels.
const PILL_TOP_GAP: f64 = 8.0;
/// Gap between the taskbar and the flyout, in logical pixels (Windows 11's own flyouts use 12).
const FLYOUT_GAP: f64 = 12.0;

/// Place the pill and show it without taking focus. The pill shows itself here because a
/// plain `show()` would activate it and pull focus away from the app being used.
pub fn pill(win: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    let Some(monitor) = win.primary_monitor()?.or(win.current_monitor()?) else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let (w, h) = ((width * scale).round(), (height * scale).round());
    let x = work.position.x as f64 + (work.size.width as f64 - w) / 2.0;
    let y = work.position.y as f64 + PILL_TOP_GAP * scale;

    win.set_focusable(false)?;
    win.set_skip_taskbar(true)?;
    win.set_always_on_top(true)?;
    win.set_size(PhysicalSize::new(w as u32, h as u32))?;
    win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;

    // If showing activates the pill anyway, hand focus back to the app that had it.
    let previous = unsafe { GetForegroundWindow() }.0 as isize;
    win.show()?;
    let pill = win.hwnd()?.0 as isize;
    win.run_on_main_thread(move || unsafe { finish_pill(HWND(pill as _), HWND(previous as _)) })
}

/// Win32 part of [`pill`]: tool window (no Alt+Tab, no taskbar button), never activated,
/// topmost; focus back to `previous` if the pill took it.
unsafe fn finish_pill(pill: HWND, previous: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(pill, GWL_EXSTYLE);
        let style = (style | (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(pill, GWL_EXSTYLE, style);
        // SWP_FRAMECHANGED applies the new style; SWP_NOACTIVATE keeps focus where it is.
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        if let Err(e) = SetWindowPos(pill, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) {
            log::warn!("pill: {e}");
        }
        if GetForegroundWindow() == pill && !previous.is_invalid() && previous != pill {
            let _ = SetForegroundWindow(previous);
        }
    }
}

/// Cover the work area of the screen the guarded app is on (the foreground window's monitor).
/// The work area leaves the taskbar out, so the taskbar stays visible and usable.
pub fn shield(win: &WebviewWindow) -> tauri::Result<()> {
    let Some(work) = foreground_work_area() else {
        return Ok(());
    };
    win.set_skip_taskbar(true)?;
    win.set_always_on_top(true)?;
    win.set_position(PhysicalPosition::new(work.x as i32, work.y as i32))?;
    win.set_size(PhysicalSize::new(work.width as u32, work.height as u32))
}

fn foreground_work_area() -> Option<Area> {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let found = unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        GetMonitorInfoW(monitor, &mut info)
    };
    let rect = info.rcWork;
    found.as_bool().then(|| Area {
        x: rect.left as f64,
        y: rect.top as f64,
        width: (rect.right - rect.left) as f64,
        height: (rect.bottom - rect.top) as f64,
    })
}

/// Open the popover as a tray flyout from whichever edge the taskbar is on.
pub fn popover(win: &WebviewWindow, icon: Rect) -> tauri::Result<()> {
    let scale = win.scale_factor()?;
    let position = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let icon = Area { x: position.x, y: position.y, width: size.width, height: size.height };

    let monitor =
        win.monitor_from_point(icon.x + icon.width / 2.0, icon.y + icon.height / 2.0)?.or(win.primary_monitor()?);
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let work = monitor.work_area();
    let work = Area {
        x: work.position.x as f64,
        y: work.position.y as f64,
        width: work.size.width as f64,
        height: work.size.height as f64,
    };
    let flyout = win.outer_size()?;
    let gap = FLYOUT_GAP * monitor.scale_factor();
    let (x, y) = flyout_origin(icon, (flyout.width as f64, flyout.height as f64), work, gap);
    win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}
