fn main() {
    // Which flavor this build is: the prod Tauri config (`--config src-tauri/tauri.prod.conf.json`,
    // which the Tauri CLI hands to build scripts as the `TAURI_CONFIG` JSON) makes the "Warden"
    // release; anything else is the "Warden β" staging build. Code reads it as `cfg(warden_prod)`:
    // the backend (config/prod.json), the extension ids and the native messaging host name.
    println!("cargo::rerun-if-env-changed=TAURI_CONFIG");
    println!("cargo::rustc-check-cfg=cfg(warden_prod)");
    if prod_identifier_in(std::env::var("TAURI_CONFIG").ok()) {
        println!("cargo::rustc-cfg=warden_prod");
    }

    // Declaring the app's commands generates one permission per command (allow-get-state, ...),
    // so each window's capability file can grant only the commands that window needs.
    let commands = &[
        "get_state",
        "sign_in_google",
        "open_get_warden",
        "open_extension_page",
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

/// True when the extra Tauri config sets the prod identifier.
fn prod_identifier_in(tauri_config: Option<String>) -> bool {
    let Some(json) = tauri_config else { return false };
    let config: serde_json::Value = serde_json::from_str(&json).expect("TAURI_CONFIG is JSON");
    config["identifier"] == "in.weenja.warden.desktop"
}
