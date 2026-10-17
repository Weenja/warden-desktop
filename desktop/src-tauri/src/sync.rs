//! Keeps this computer in step with the server: device registration, heartbeat, wallet,
//! balance token and the block list.
//!
//! The drain loop plugs in here: it reads [`SyncService::guarded_apps`] /
//! [`SyncService::guarded_sites`] and the local balance, and charges with
//! [`SyncService::spend_tick`]. The server stays authoritative; the balance token only lets
//! the app keep counting down (and shield at zero) between ticks or while offline.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::api::{
    Api, ApiError, BlockTarget, GuardedItem, HeartbeatRequest, RegisterDevice, SpendRequest, SpendResponse,
    SpendTarget, Wallet,
};
use crate::config::APP_VERSION;
use crate::drain::GuardedApp;
use crate::guard::{DrainView, GuardView, ShieldView};
use crate::platform::{self, Platform, KNOWN_BROWSERS};
use crate::session::{now_secs, SessionError, SessionManager};

/// The crew sees a device that stays silent for hours as "went dark". One heartbeat a minute
/// is far inside that and far below the server's rate limit (120 per 10 min).
const HEARTBEAT_EVERY: Duration = Duration::from_secs(60);
/// How often the loop wakes to look for sleep/wake and lock/unlock.
const POLL_EVERY: Duration = Duration::from_secs(5);
/// Crew names change rarely; no need to ask every minute.
const CREW_EVERY: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("this computer is not registered yet")]
    NotLinked,
}

impl SyncError {
    pub fn is_signed_out(&self) -> bool {
        matches!(self, SyncError::Session(SessionError::SignedOut))
    }
}

/// Claims of the server-signed balance token (HS256 JWT, 15 min TTL). We cannot verify the
/// signature (the secret stays on the server) and do not need to: it came over TLS from the
/// server, and every charge is re-checked server-side.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BalanceClaims {
    pub sub: String,
    pub balance_seconds: i64,
    pub day_key: String,
    pub multiplier: f64,
    pub iat: i64,
    pub exp: i64,
}

impl BalanceClaims {
    pub fn decode(token: &str) -> Option<BalanceClaims> {
        let payload = token.split('.').nth(1)?;
        let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Past `exp` the app must not trust the number any more: shield until the server answers.
    pub fn expired(&self, now: i64) -> bool {
        now >= self.exp
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkedDevice {
    pub id: String,
    pub name: String,
}

/// An installed app as the UI shows it (browsers, the picker, the pill).
#[derive(Debug, Clone, Serialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    /// `data:image/png;base64,...`, so the UI needs no file access.
    pub icon: Option<String>,
}

/// An installed browser for the popover, and whether the Warden extension is linked in it.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserView {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub extension: bool,
}

/// A guarded app of this computer, with its icon when the app is installed.
#[derive(Debug, Clone, Serialize)]
pub struct GuardedAppView {
    pub token: String,
    pub label: String,
    pub icon: Option<String>,
}

#[derive(Default)]
struct SyncState {
    device: Option<LinkedDevice>,
    wallet: Option<Wallet>,
    balance: Option<BalanceClaims>,
    targets: Vec<BlockTarget>,
    crew: Vec<String>,
    /// Installed apps, read from disk once at start and again when the picker opens.
    apps: Vec<AppEntry>,
    /// False after a failed request, true after the next good one. Shown as "offline".
    online: bool,
}

/// What the UI windows render. Never contains tokens.
#[derive(Debug, Clone, Serialize)]
pub struct UiState {
    pub signed_in: bool,
    pub email: Option<String>,
    pub device: Option<LinkedDevice>,
    /// The local, optimistic balance while the drain knows it (it counts down between ticks);
    /// the server's otherwise.
    pub balance_seconds: Option<i64>,
    pub daily_max_seconds: Option<i64>,
    pub low_balance: bool,
    pub guarded_apps: Vec<GuardedAppView>,
    pub guarded_sites: usize,
    pub crew: Vec<String>,
    pub browsers: Vec<BrowserView>,
    pub online: bool,
    /// What drains now (the pill). None = the pill is hidden.
    pub drain: Option<DrainView>,
    /// The shield, when a guarded app is in front at zero.
    pub shield: Option<ShieldView>,
}

pub struct SyncService {
    api: Api,
    session: Arc<SessionManager>,
    platform: Arc<dyn Platform>,
    install_id: String,
    state: Mutex<SyncState>,
}

impl SyncService {
    pub fn new(api: Api, session: Arc<SessionManager>, platform: Arc<dyn Platform>, data_dir: &Path) -> SyncService {
        SyncService {
            api,
            session,
            platform,
            install_id: load_or_create_install_id(data_dir),
            state: Mutex::new(SyncState { online: true, ..Default::default() }),
        }
    }

    pub fn session(&self) -> &Arc<SessionManager> {
        &self.session
    }

    pub fn platform(&self) -> &Arc<dyn Platform> {
        &self.platform
    }

    pub fn device_id(&self) -> Option<String> {
        self.state.lock().unwrap().device.as_ref().map(|d| d.id.clone())
    }

    /// The latest balance token claims, for the drain loop's local countdown.
    pub fn balance(&self) -> Option<BalanceClaims> {
        self.state.lock().unwrap().balance.clone()
    }

    /// Guarded apps for THIS computer (apps are per device; the picker sets them).
    pub fn guarded_apps(&self) -> Vec<BlockTarget> {
        let state = self.state.lock().unwrap();
        let me = state.device.as_ref().map(|d| d.id.as_str());
        state
            .targets
            .iter()
            .filter(|t| t.kind == "app" && t.device_id.as_deref() == me && me.is_some())
            .cloned()
            .collect()
    }

    /// Guarded apps joined to their block-list rows, as the drain needs them.
    pub fn guarded_for_drain(&self) -> Vec<GuardedApp> {
        self.guarded_apps()
            .into_iter()
            .map(|t| GuardedApp { app_id: t.token, target_id: t.id, label: t.label })
            .collect()
    }

    /// Guarded sites, set on the phone for every device.
    pub fn guarded_sites(&self) -> Vec<BlockTarget> {
        self.state.lock().unwrap().targets.iter().filter(|t| t.kind == "site").cloned().collect()
    }

    /// Run `call` with a valid access token. On 401 (token revoked or clock skew) refresh once
    /// and retry. Records online/offline for the UI.
    async fn authed<T, F, Fut>(&self, call: F) -> Result<T, SyncError>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let token = self.session.access_token().await?;
        let result = match call(token.clone()).await {
            Err(ApiError::Http { status: 401, .. }) => {
                let fresh = self.session.refresh_after_rejection(&token).await?;
                call(fresh).await
            }
            other => other,
        };
        self.state.lock().unwrap().online = !matches!(result, Err(ApiError::Network(_)));
        Ok(result?)
    }

    /// Create or refresh this computer's device row (idempotent per install id).
    pub async fn register_device(&self) -> Result<LinkedDevice, SyncError> {
        let name = self.platform.device_name();
        let args = RegisterDevice {
            p_platform: platform::device_platform(),
            p_name: &name,
            p_install_id: &self.install_id,
            p_push_token: None,
            p_app_version: APP_VERSION,
        };
        let api = &self.api;
        let args = &args;
        let device = self.authed(|jwt| async move { api.register_device(&jwt, args).await }).await?;
        let linked = LinkedDevice { id: device.id, name: device.name.unwrap_or(name) };
        self.state.lock().unwrap().device = Some(linked.clone());
        Ok(linked)
    }

    /// Send a heartbeat. `stopped_at` (ISO time) is set only by Quit, so the crew is told.
    pub async fn heartbeat(&self, stopped_at: Option<&str>) -> Result<(), SyncError> {
        let device_id = self.device_id().ok_or(SyncError::NotLinked)?;
        let body = HeartbeatRequest {
            device_id: &device_id,
            blocking_enabled: true,
            screen_time_permission: true,
            health_permission: false,
            app_version: APP_VERSION,
            stopped_at,
        };
        let api = &self.api;
        let body = &body;
        let res = self.authed(|jwt| async move { api.heartbeat(&jwt, body).await }).await?;
        self.store_wallet(res.wallet, &res.balance_token);
        Ok(())
    }

    /// Charge `seconds` of foreground time. `evidence` must be unique per tick and reused on
    /// retry, so a retried tick is never charged twice.
    pub async fn spend_tick(
        &self,
        target: &SpendTarget,
        seconds: u32,
        evidence: &str,
    ) -> Result<SpendResponse, SyncError> {
        let device_id = self.device_id().ok_or(SyncError::NotLinked)?;
        let (target_id, site) = match target {
            SpendTarget::Target(id) => (Some(id.as_str()), None),
            SpendTarget::Site(host) => (None, Some(host.as_str())),
            SpendTarget::WholeBrowser => (None, None),
        };
        let body = SpendRequest { device_id: &device_id, target_id, site, seconds, evidence };
        let api = &self.api;
        let body = &body;
        let res = self.authed(|jwt| async move { api.spend_tick(&jwt, body).await }).await?;
        self.store_wallet(res.wallet.clone(), &res.balance_token);
        Ok(res)
    }

    pub async fn refresh_block_list(&self) -> Result<(), SyncError> {
        let api = &self.api;
        let targets = self.authed(|jwt| async move { api.block_targets(&jwt).await }).await?;
        self.state.lock().unwrap().targets = targets;
        Ok(())
    }

    /// Replace the guarded apps of this computer (the picker's "Done").
    pub async fn set_guarded_apps(&self, items: &[GuardedItem]) -> Result<(), SyncError> {
        let device_id = self.device_id().ok_or(SyncError::NotLinked)?;
        let api = &self.api;
        let device_id = &device_id;
        self.authed(|jwt| async move { api.set_guarded(&jwt, "app", items, Some(device_id)).await }).await?;
        self.refresh_block_list().await
    }

    pub async fn refresh_crew(&self) -> Result<(), SyncError> {
        let api = &self.api;
        let crew = self.authed(|jwt| async move { api.crew_state(&jwt).await }).await?;
        let names = crew.members.unwrap_or_default().into_iter().filter(|m| !m.me).map(|m| m.name).collect();
        self.state.lock().unwrap().crew = names;
        Ok(())
    }

    /// Read the installed apps (browsers, picker, icons). Reads the disk: call it off the UI
    /// thread. The list stays on this computer; only apps the user picks are sent.
    pub fn scan_apps(&self) {
        let apps = self
            .platform
            .installed_apps()
            .into_iter()
            .map(|app| AppEntry {
                icon: app.icon_png.map(|png| format!("data:image/png;base64,{}", STANDARD.encode(png))),
                id: app.id,
                name: app.name,
            })
            .collect();
        self.state.lock().unwrap().apps = apps;
    }

    pub fn installed_apps(&self) -> Vec<AppEntry> {
        self.state.lock().unwrap().apps.clone()
    }

    /// The icon of an installed app, for the pill and the popover.
    pub fn app_icon(&self, id: &str) -> Option<String> {
        self.state.lock().unwrap().apps.iter().find(|a| a.id == id).and_then(|a| a.icon.clone())
    }

    fn store_wallet(&self, wallet: Wallet, token: &str) {
        let mut state = self.state.lock().unwrap();
        state.balance = BalanceClaims::decode(token);
        state.wallet = Some(wallet);
    }

    /// Forget everything that belongs to the signed-out user.
    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap();
        let apps = std::mem::take(&mut state.apps);
        *state = SyncState { online: true, apps, ..Default::default() };
    }

    /// What the windows render: the synced state plus what the drain loop sees (`guard`) and
    /// the browsers whose Warden extension is linked (`linked_browsers`).
    pub async fn ui_state(&self, guard: &GuardView, linked_browsers: &[String]) -> UiState {
        let signed_in = self.session.is_signed_in().await;
        let email = self.session.email().await;
        let guarded_apps: Vec<GuardedAppView> = self
            .guarded_apps()
            .into_iter()
            .map(|t| GuardedAppView { icon: self.app_icon(&t.token), token: t.token, label: t.label })
            .collect();
        let guarded_sites = self.guarded_sites().len();
        let state = self.state.lock().unwrap();
        let browsers = state
            .apps
            .iter()
            .filter(|a| KNOWN_BROWSERS.contains(&a.id.as_str()))
            .map(|a| BrowserView {
                id: a.id.clone(),
                name: a.name.clone(),
                icon: a.icon.clone(),
                extension: linked_browsers.contains(&a.id),
            })
            .collect();
        UiState {
            signed_in,
            email,
            device: state.device.clone(),
            balance_seconds: guard.balance_seconds.or(state.wallet.as_ref().map(|w| w.balance_seconds)),
            daily_max_seconds: state.wallet.as_ref().and_then(|w| w.daily_max_seconds),
            low_balance: state.wallet.as_ref().is_some_and(|w| w.low_balance),
            guarded_apps,
            guarded_sites,
            crew: state.crew.clone(),
            browsers,
            online: state.online,
            drain: guard.drain.clone(),
            shield: guard.shield.clone(),
        }
    }

    /// Everything a fresh sign-in or app start needs before the popover makes sense.
    pub async fn full_sync(&self) -> Result<(), SyncError> {
        self.register_device().await?;
        self.heartbeat(None).await?;
        self.refresh_block_list().await?;
        // The crew line is decoration; a failure here must not block the rest.
        if let Err(e) = self.refresh_crew().await {
            log::warn!("crew refresh failed: {e}");
        }
        Ok(())
    }

    /// Heartbeat every minute, and at once after the Mac wakes or the screen unlocks (the
    /// crew should not see a stale "last seen" after a lid close). `on_change` is called after
    /// each sync so the UI can refresh; it gets the error, if any.
    pub async fn run(self: Arc<Self>, on_change: impl Fn(Option<&SyncError>)) {
        let mut last_beat = Instant::now();
        let mut last_crew = Instant::now();
        let mut was_locked = self.platform.screen_locked();
        let mut wall = SystemTime::now();
        let mut mono = Instant::now();
        loop {
            tokio::time::sleep(POLL_EVERY).await;
            let woke = slept_between(wall, mono);
            wall = SystemTime::now();
            mono = Instant::now();
            let locked = self.platform.screen_locked();
            let unlocked = was_locked && !locked;
            was_locked = locked;

            if !(woke || unlocked || last_beat.elapsed() >= HEARTBEAT_EVERY) || !self.session.is_signed_in().await {
                continue;
            }
            last_beat = Instant::now();
            let mut result = self.sync_once().await;
            if result.is_ok() && last_crew.elapsed() >= CREW_EVERY {
                last_crew = Instant::now();
                result = self.refresh_crew().await;
            }
            on_change(result.as_ref().err());
        }
    }

    async fn sync_once(&self) -> Result<(), SyncError> {
        if self.device_id().is_none() {
            self.register_device().await?;
        }
        self.heartbeat(None).await?;
        self.refresh_block_list().await
    }
}

/// True when the wall clock moved much more than the monotonic clock: the machine slept
/// (on macOS the monotonic clock stops during sleep).
fn slept_between(wall_before: SystemTime, mono_before: Instant) -> bool {
    let wall = SystemTime::now().duration_since(wall_before).unwrap_or_default();
    wall.saturating_sub(mono_before.elapsed()) > Duration::from_secs(30)
}

/// A random id for this installation, kept in the app data folder. It is not secret: it only
/// lets `register_device` find the same device row again after a restart or re-sign-in.
fn load_or_create_install_id(dir: &Path) -> String {
    let path: PathBuf = dir.join("install_id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        if uuid::Uuid::parse_str(id.trim()).is_ok() {
            return id.trim().to_string();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, &id)) {
        log::warn!("could not save install id: {e}");
    }
    id
}

/// Seconds since the Unix epoch as an RFC 3339 UTC string, for `stopped_at`.
pub fn iso_now() -> String {
    iso_from_unix(now_secs())
}

fn iso_from_unix(secs: i64) -> String {
    // Civil-from-days (Howard Hinnant). Avoids a date crate for one timestamp.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_balance_token_payload() {
        let payload = URL_SAFE_NO_PAD.encode(
            br#"{"sub":"u1","balance_seconds":2520,"day_key":"2026-10-17","multiplier":1,"iat":100,"exp":1000}"#,
        );
        let token = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig");
        let claims = BalanceClaims::decode(&token).unwrap();
        assert_eq!(claims.balance_seconds, 2520);
        assert!(!claims.expired(999));
        assert!(claims.expired(1000));
        assert_eq!(BalanceClaims::decode("not-a-jwt"), None);
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_792_195_200), "2026-10-17T00:00:00Z");
        assert_eq!(iso_from_unix(951_825_599), "2000-02-29T11:59:59Z");
    }

    #[test]
    fn install_id_survives_restart() {
        let dir = std::env::temp_dir().join(format!("warden-test-{}", uuid::Uuid::new_v4()));
        let first = load_or_create_install_id(&dir);
        assert_eq!(load_or_create_install_id(&dir), first);
        std::fs::remove_dir_all(dir).ok();
    }
}
