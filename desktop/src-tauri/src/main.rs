// No console window behind the app in Windows release builds. (The browser's native messaging
// host is the separate console program `src/bin/warden-native-host.rs`.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    warden_desktop_lib::run()
}
