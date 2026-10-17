fn main() {
    // Declaring the app's commands generates one permission per command (allow-get-state, ...),
    // so each window's capability file can grant only the commands that window needs.
    let commands = &[
        "get_state",
        "sign_in_google",
        "open_get_warden",
        "send_email_code",
        "verify_email_code",
        "fit_window",
        "open_signin",
        "close_signin",
        "quit_warden",
        "pill_fit",
        "pill_metrics",
        "stop_draining",
        "close_locked_app",
        "list_apps",
        "save_guarded_apps",
        "open_picker",
        "open_quit",
        "close_window",
        "get_autostart",
        "set_autostart",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("tauri build step failed");
}
