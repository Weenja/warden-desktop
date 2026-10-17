fn main() {
    // Declaring the app's commands generates one permission per command (allow-get-state, ...),
    // so each window's capability file can grant only the commands that window needs.
    let commands = &[
        "get_state",
        "sign_in_google",
        "send_email_code",
        "verify_email_code",
        "fit_window",
        "open_signin",
        "close_signin",
        "quit_warden",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("tauri build step failed");
}
