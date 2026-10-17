//! Warden desktop: wiring. Startup, plugins, tray, deep links and the background sync loop.

pub mod api;
pub mod auth;
mod commands;
pub mod config;
pub mod platform;
pub mod session;
pub mod sync;
mod tray;
mod ui_windows;

use std::sync::Arc;

use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::api::Api;
use crate::auth::PendingSignIn;
use crate::config::Config;
use crate::session::{KeychainStore, RefreshFn, SecretStore, SessionManager};
use crate::sync::SyncService;

/// Shared state behind every command and background task.
pub struct AppState {
    pub api: Api,
    pub sync: Arc<SyncService>,
    pub pending: PendingSignIn,
}

/// The session manager with refresh through the Warden API. The app stores the session in the
/// OS keychain ([`KeychainStore`]); the staging tests pass an in-memory store.
pub fn session_manager_with(api: &Api, store: Box<dyn SecretStore>) -> SessionManager {
    let refresh_api = api.clone();
    let refresh: RefreshFn = Arc::new(move |token: String| {
        let api = refresh_api.clone();
        Box::pin(async move { api.refresh(&token).await })
    });
    SessionManager::new(store, refresh)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Warnings to stderr only (RUST_LOG=info for more). Nothing is logged to files or sent anywhere.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let app = tauri::Builder::default()
        // Must be first: a second launch (or, on Windows, a warden:// link that starts a new
        // process) hands its arguments to the running app and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if !app.state::<AppState>().sync.session().is_signed_in().await {
                    let _ = ui_windows::open_signin(&app);
                }
            });
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::sign_in_google,
            commands::send_email_code,
            commands::verify_email_code,
            commands::fit_window,
            commands::open_signin,
            commands::close_signin,
            commands::quit_warden,
        ])
        .setup(|app| {
            // Menu bar app: no Dock icon, no app menu.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config = Config::staging();
            let api = Api::new(&config);
            let identifier = app.config().identifier.clone();
            let session = Arc::new(session_manager_with(&api, Box::new(KeychainStore::new(&identifier))));
            let platform: Arc<dyn platform::Platform> = Arc::from(platform::current());
            let data_dir = app.path().app_data_dir()?;
            let sync = Arc::new(SyncService::new(api.clone(), session, platform, &data_dir));
            app.manage(AppState { api, sync, pending: PendingSignIn::default() });

            let handle = app.handle().clone();
            tray::create(&handle)?;
            ui_windows::create_popover(&handle)?;
            listen_for_deep_links(&handle)?;
            tauri::async_runtime::spawn(start(handle));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Warden failed to start");

    app.run(|_app, event| {
        // Closing the last window must not quit: Warden lives in the menu bar. Only an
        // explicit exit (Quit Warden) carries an exit code.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}

fn listen_for_deep_links(app: &AppHandle) -> tauri::Result<()> {
    // In a dev build on Windows the scheme is not installed yet; register it for this exe.
    #[cfg(all(windows, debug_assertions))]
    app.deep_link().register_all().map_err(|e| tauri::Error::Anyhow(e.into()))?;

    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            let app = handle.clone();
            let url = url.to_string();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                commands::handle_auth_callback(&app, &state, &url).await;
            });
        }
    });
    Ok(())
}

/// Load the saved session; sync if signed in, otherwise ask to sign in. Then keep syncing.
async fn start(app: AppHandle) {
    let state = app.state::<AppState>();
    let sync = state.sync.clone();

    let signed_in = sync.session().load().await.unwrap_or_else(|e| {
        log::warn!("could not read the saved session: {e}");
        false
    });
    if signed_in {
        if let Err(e) = sync.full_sync().await {
            log::warn!("first sync failed: {e}");
            if e.is_signed_out() {
                let _ = ui_windows::open_signin(&app);
            }
        }
    } else {
        let _ = ui_windows::open_signin(&app);
    }
    commands::emit_state(&app, &state).await;

    let scan = sync.clone();
    let scan_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        scan.scan_browsers();
        tauri::async_runtime::block_on(async {
            commands::emit_state(&scan_app, &scan_app.state::<AppState>()).await;
        });
    });

    let loop_app = app.clone();
    sync.run(move |error| {
        let app = loop_app.clone();
        let signed_out = error.is_some_and(|e| e.is_signed_out());
        if let Some(e) = error {
            log::warn!("sync: {e}");
        }
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            if signed_out {
                state.sync.clear();
                let _ = ui_windows::open_signin(&app);
            }
            commands::emit_state(&app, &state).await;
        });
    })
    .await;
}
