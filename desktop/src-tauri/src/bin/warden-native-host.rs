//! `warden-native-host`: the program Chrome, Edge, Firefox and the other browsers start when
//! the Warden extension connects (native messaging). It only relays messages between the
//! browser (stdin/stdout) and the running Warden app; see `browser_ext::host`.
//!
//! It is a separate CONSOLE program, not the app itself: Chrome on Windows starts hosts through
//! `cmd /c` with pipes for stdin/stdout, and a GUI-subsystem exe (the app, in release builds)
//! may not keep them, so the browser would report "Native host has exited". The same helper is
//! used on macOS too, so there is one code path. Tauri ships it next to the app.

fn main() {
    std::process::exit(warden_desktop_lib::browser_ext::host::run())
}
