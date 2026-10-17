// No console window behind the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started by a browser as the native messaging host: relay for the extension, no window.
    let args: Vec<String> = std::env::args().collect();
    if warden_desktop_lib::browser_ext::host::is_host_launch(&args) {
        std::process::exit(warden_desktop_lib::browser_ext::host::run());
    }
    warden_desktop_lib::run()
}
