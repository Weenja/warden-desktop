//! The only calls the UI can make into the core. Narrow on purpose: the web views never see a
//! token, a URL with secrets, or raw server errors; they get [`UiState`] and short messages.
//! Which window may call which command is set in `capabilities/`.

use std::time::Duration;

use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use tauri_plugin_autostart::ManagerExt as _;

use crate::api::{ApiError, GuardedItem, TokenResponse};
use crate::auth::{self, looks_like_email, Claim};
use crate::browser_ext::link::{Draining, Facts};
use crate::drain::{DrainKind, LOW_POINTS};
use crate::guard::GuardView;
use crate::new_account::{self, Screened};
use crate::picker::{picker_apps, PickerApp};
use crate::placement::{self, PillMetrics};
use crate::session::{now_secs, Session, SessionError};
use crate::sync::{iso_now, SyncError, UiState};
use crate::{ui_windows, AppState};

/// Event the UI listens to for a fresh [`UiState`].
pub const STATE_EVENT: &str = "state-changed";
/// Event telling the sign-in window how a browser sign-in ended.
pub const AUTH_EVENT: &str = "auth-result";

/// Where "getwarden.app" on the sign-in window goes. Fixed here, so the UI cannot open other URLs.
/// `scripts/make-qr.py` reads this line for the QR code, so both always point to the same place.
const GET_WARDEN_URL: &str = "https://getwarden.app";

#[derive(Clone, serde::Serialize)]
pub struct AuthResult {
    pub ok: bool,
    pub message: Option<String>,
    /// Set when the Google account has no Warden account: the sign-in window shows
    /// "No Warden account for <email>". `Some("")` when Google gave no email.
    pub no_account: Option<String>,
}

#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Result<UiState, String> {
    Ok(ui_state(&state).await)
}

async fn ui_state(state: &AppState) -> UiState {
    state.sync.ui_state(&state.guard.view(), &state.links.linked_browsers()).await
}

/// Open Google sign-in in the user's default browser. It returns through the deep link.
/// `select_account`: make Google show its account chooser ("Use another account").
#[tauri::command]
pub fn sign_in_google(app: AppHandle, state: State<'_, AppState>, select_account: Option<bool>) -> Result<(), String> {
    let pkce = state.pending.start();
    let url = auth::authorize_url(
        state.api.base_url(),
        &state.auth_return_url,
        "google",
        &pkce.challenge,
        select_account.unwrap_or(false),
    );
    app.opener().open_url(url.as_str(), None::<&str>).map_err(|_| "Could not open your browser.".to_string())
}

/// "getwarden.app" under the QR code: the website, in the user's browser.
#[tauri::command]
pub fn open_get_warden(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(GET_WARDEN_URL, None::<&str>).map_err(|_| "Could not open your browser.".to_string())
}

#[tauri::command]
pub async fn send_email_code(state: State<'_, AppState>, email: String) -> Result<(), String> {
    let email = email.trim().to_lowercase();
    if !looks_like_email(&email) {
        return Err("That doesn't look like an email address.".into());
    }
    state.api.send_email_code(&email).await.map_err(|e| friendly_auth_error(&e))
}

#[tauri::command]
pub async fn verify_email_code(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    code: String,
) -> Result<(), String> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        return Err("Type all 6 digits.".into());
    }
    let tokens = state.api.verify_email_code(email.trim(), &code).await.map_err(|e| friendly_auth_error(&e))?;
    finish_sign_in(&app, &state, tokens).await
}

/// Size a window to its content. The page measures itself after layout and reports here,
/// so the window never shows scroll bars or empty space.
///
/// `viewport` is the height the page can see right now. On macOS the title bar sits over
/// the top of the web view, so the window's inner height is taller than the page's viewport
/// (about 30 px on a titled window, 0 on borderless ones). We add that difference, or the
/// last line of the page would be cut off.
#[tauri::command]
pub fn fit_window(window: WebviewWindow, height: f64, viewport: f64) -> Result<(), String> {
    let inner = window.inner_size().map_err(|e| e.to_string())?.to_logical::<f64>(window.scale_factor().unwrap_or(1.0));
    // Clamped: while a resize is still settling the two numbers can disagree for a moment.
    let title_bar = (inner.height - viewport).clamp(0.0, 80.0);
    let height = (height + title_bar).clamp(60.0, 900.0);
    window.set_size(LogicalSize::new(inner.width, height)).map_err(|e| e.to_string())
}

/// "Sign in" from the popover when the session is gone.
#[tauri::command]
pub fn open_signin(app: AppHandle) -> Result<(), String> {
    ui_windows::open_signin(&app).map_err(|e| e.to_string())
}

/// "Done" on the linked screen: the window goes, Warden stays in the menu bar.
#[tauri::command]
pub fn close_signin(app: AppHandle) {
    ui_windows::close_signin(&app);
}

/// The pill page measured itself.
#[tauri::command]
pub fn pill_fit(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    ui_windows::fit_pill(&app, width.clamp(40.0, 600.0), height.clamp(20.0, 600.0)).map_err(|e| e.to_string())
}

/// Notch width and menu bar height, so the pill lines up with the notch.
#[tauri::command]
pub fn pill_metrics(window: WebviewWindow) -> PillMetrics {
    placement::pill_metrics(&window)
}

/// Stop on the pill. An app is asked to quit (the polite quit: it may ask to save). A guarded
/// site's tab is closed by the browser extension. A whole browser is left open (closing every
/// tab would be too much): its pill hides, it keeps draining while it is in front.
#[tauri::command]
pub async fn stop_draining(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(drain) = state.guard.view().drain else {
        return Ok(());
    };
    match drain.kind {
        DrainKind::App => {
            state.sync.platform().terminate_app(&drain.app_id);
        }
        DrainKind::Site => state.links.close_tab(&drain.app_id, &drain.label),
        DrainKind::Browser => {
            state.guard.hide_pill(&drain.app_id);
            if let Some(pill) = app.get_webview_window(ui_windows::PILL) {
                let _ = pill.hide();
            }
        }
    }
    Ok(())
}

/// "Close <app>" on the shield: ask the locked app to quit.
#[tauri::command]
pub async fn close_locked_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(shield) = state.guard.view().shield else {
        return Ok(());
    };
    state.guard.closing(&shield.app_id);
    if let Some(win) = app.get_webview_window(ui_windows::SHIELD) {
        let _ = win.hide();
    }
    state.sync.platform().terminate_app(&shield.app_id);
    Ok(())
}

/// The picker's list. Reads the disk again (apps may have been installed since start), off
/// the main thread.
#[tauri::command]
pub async fn list_apps(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<PickerApp>, String> {
    let sync = state.sync.clone();
    tauri::async_runtime::spawn_blocking(move || sync.scan_apps()).await.map_err(|e| e.to_string())?;
    let guarded: Vec<GuardedItem> =
        state.sync.guarded_apps().into_iter().map(|t| GuardedItem { token: t.token, label: t.label }).collect();
    Ok(picker_apps(&state.sync.installed_apps(), &guarded, &app.config().identifier))
}

/// "Done" in the picker: these apps are the guarded apps of this computer from now on.
#[tauri::command]
pub async fn save_guarded_apps(
    app: AppHandle,
    state: State<'_, AppState>,
    items: Vec<GuardedItem>,
) -> Result<(), String> {
    state.sync.set_guarded_apps(&items).await.map_err(|e| match e {
        SyncError::Api(ApiError::Network(_)) => "No connection. Your picks were not saved.".to_string(),
        _ => "Could not save your picks. Try again.".to_string(),
    })?;
    emit_state(&app, &state).await;
    Ok(())
}

#[tauri::command]
pub fn open_picker(app: AppHandle) -> Result<(), String> {
    ui_windows::open_picker(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_quit(app: AppHandle) -> Result<(), String> {
    ui_windows::open_quit(&app).map_err(|e| e.to_string())
}

/// A dialog's own close button ("Keep Warden on", "Done").
#[tauri::command]
pub fn close_window(window: WebviewWindow) {
    let _ = window.close();
}

/// "Open at login": a login item (LaunchAgent on macOS, Run key on Windows) that starts Warden.
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    let autolaunch = app.autolaunch();
    if on { autolaunch.enable() } else { autolaunch.disable() }
        .map_err(|_| "Could not change the login item.".to_string())?;
    autolaunch.is_enabled().map_err(|e| e.to_string())
}

/// Quit Warden. The crew is told: a last heartbeat carries `stopped_at`, as on Android.
#[tauri::command]
pub async fn quit_warden(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let stopped_at = iso_now();
    // Do not hang the quit on a dead network; the server also notices the silence later.
    let told = tokio::time::timeout(Duration::from_secs(5), state.sync.heartbeat(Some(&stopped_at))).await;
    if !matches!(told, Ok(Ok(()))) {
        log::warn!("quit heartbeat did not go through");
    }
    app.exit(0);
    Ok(())
}

/// Shared end of every sign-in path: keep the session, register this computer, sync.
pub async fn finish_sign_in(app: &AppHandle, state: &AppState, tokens: TokenResponse) -> Result<(), String> {
    state.sync.session().set(Session::from_token_response(tokens)).await.map_err(|e| match e {
        SessionError::Keychain(_) => "Could not save your sign-in to the keychain.".to_string(),
        other => other.to_string(),
    })?;
    state.sync.full_sync().await.map_err(|e| friendly_sync_error(&e))?;
    // A guard is only useful if it runs: open at login by default. The linked screen shows the
    // switch right away. Not in debug builds, which would register a dev binary.
    if !cfg!(debug_assertions) {
        if let Err(e) = app.autolaunch().enable() {
            log::warn!("could not turn on open at login: {e}");
        }
    }
    emit_state(app, state).await;
    Ok(())
}

/// How a Google sign-in from the deep link ended.
enum GoogleOutcome {
    SignedIn,
    /// The Google account had no Warden account; the empty one the sign-in made is deleted.
    NoAccount(Option<String>),
    Failed(String),
}

/// Finish a Google sign-in from the `warden://auth-callback` deep link.
pub async fn handle_auth_callback(app: &AppHandle, state: &AppState, url: &str) {
    log::info!("deep link received");
    let outcome = match auth::parse_callback(url) {
        Err(auth::CallbackError::NotOurs) => return,
        Err(e) => GoogleOutcome::Failed(e.to_string()),
        Ok(code) => match state.pending.claim(&code) {
            Claim::Fresh(verifier) => match state.api.exchange_pkce(&code, &verifier).await {
                Ok(tokens) => finish_google_sign_in(app, state, tokens).await,
                Err(e) => GoogleOutcome::Failed(friendly_auth_error(&e)),
            },
            // The web page's "Open Warden" button sent the same link again: just show Warden.
            Claim::Repeat => return bring_forward(app, state).await,
            // A stale link while signed in (e.g. that button after a restart): no error either.
            Claim::Expired if state.sync.session().is_signed_in().await => return bring_forward(app, state).await,
            Claim::Expired => GoogleOutcome::Failed("This sign-in link has expired. Try again.".to_string()),
        },
    };
    let payload = match outcome {
        GoogleOutcome::SignedIn => AuthResult { ok: true, message: None, no_account: None },
        GoogleOutcome::NoAccount(email) => {
            log::info!("google sign-in: no Warden account; the empty one was deleted");
            AuthResult { ok: false, message: None, no_account: Some(email.unwrap_or_default()) }
        }
        GoogleOutcome::Failed(message) => AuthResult { ok: false, message: Some(message), no_account: None },
    };
    if let Some(message) = &payload.message {
        log::warn!("sign-in link: {message}");
    }
    let _ = ui_windows::open_signin(app);
    let _ = app.emit_to(ui_windows::SIGNIN, AUTH_EVENT, payload);
}

/// Show Warden without a sign-in result: the popover when signed in, else the sign-in window
/// (which keeps waiting for the sign-in in progress).
async fn bring_forward(app: &AppHandle, state: &AppState) {
    let shown = if state.sync.session().is_signed_in().await {
        ui_windows::open_popover(app)
    } else {
        ui_windows::open_signin(app)
    };
    if let Err(e) = shown {
        log::warn!("show app: {e}");
    }
}

/// Google can create an account during sign-in (email cannot: `create_user: false`). Check for
/// that BEFORE anything is saved or registered; see `new_account.rs` for the safety rule.
async fn finish_google_sign_in(app: &AppHandle, state: &AppState, tokens: TokenResponse) -> GoogleOutcome {
    match new_account::screen_google_sign_in(&state.api, &tokens, now_secs()).await {
        Ok(Screened::Kept) => match finish_sign_in(app, state, tokens).await {
            Ok(()) => GoogleOutcome::SignedIn,
            Err(message) => GoogleOutcome::Failed(message),
        },
        Ok(Screened::Deleted) => {
            // The session was never saved; clearing makes sure no token of the deleted account
            // is left in the keychain or in memory.
            if let Err(e) = state.sync.session().sign_out().await {
                log::warn!("could not clear the session: {e}");
            }
            GoogleOutcome::NoAccount(tokens.user.email)
        }
        // Nothing was saved and nothing deleted. A retry within 10 minutes checks again.
        Err(e) => GoogleOutcome::Failed(friendly_auth_error(&e)),
    }
}

/// The state changed: re-render every window and tell every linked browser extension.
pub async fn emit_state(app: &AppHandle, state: &AppState) {
    let guard = state.guard.view();
    let ui = state.sync.ui_state(&guard, &state.links.linked_browsers()).await;
    let sites = state.sync.guarded_sites().into_iter().map(|t| t.token).collect();
    state.links.publish(extension_facts(&ui, &guard, sites));
    let _ = app.emit(STATE_EVENT, ui);
}

/// The `state` message for the extensions: only what they need to show the badge, the popup
/// and the blocked page. It never leaves this computer.
fn extension_facts(ui: &UiState, guard: &GuardView, guarded_sites: Vec<String>) -> Facts {
    let points = ui.balance_seconds.unwrap_or(0).max(0) / 60;
    let draining = guard.drain.as_ref().filter(|d| d.kind == DrainKind::Site && !d.locked).map(|d| Draining {
        browser: d.app_id.clone(),
        site: d.label.clone(),
        per_min: d.per_minute,
    });
    Facts {
        signed_in: ui.signed_in,
        points,
        low: ui.signed_in && points <= LOW_POINTS,
        locked: guard.locked,
        guarded_sites,
        draining,
        device_name: ui.device.as_ref().map(|d| d.name.clone()),
    }
}

fn friendly_auth_error(e: &ApiError) -> String {
    match e {
        ApiError::Network(_) => "No connection. Check your internet and try again.".into(),
        ApiError::Http { status: 429, .. } => "Too many tries. Wait a minute, then try again.".into(),
        ApiError::Http { code, .. } if code == "otp_expired" => "That code is wrong or expired.".into(),
        ApiError::Http { code, .. }
            if code == "otp_disabled" || code == "signup_disabled" || code == "user_not_found" =>
        {
            "No Warden account uses this email. Get Warden on your phone first.".into()
        }
        ApiError::Http { status: 400..=499, .. } => "Sign-in didn't work. Try again.".into(),
        _ => "Warden's server had a problem. Try again in a minute.".into(),
    }
}

fn friendly_sync_error(e: &SyncError) -> String {
    match e {
        SyncError::Api(ApiError::Network(_)) => "Signed in, but no connection to link this computer. Try again.".into(),
        _ => "Signed in, but linking this computer failed. Try again.".into(),
    }
}
