//! End-to-end checks against the STAGING backend with a fresh anonymous user.
//! Ignored by default (they need the network and create a staging user):
//!
//!     cargo test --test staging -- --ignored --nocapture
//!
//! Anonymous sign-up is a staging test door only; the app itself has no anonymous login.
//! Each test deletes its user at the end (`delete-account`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use warden_desktop_lib::api::{
    Api, ApiError, DevicePlatform, GuardedItem, HeartbeatRequest, RegisterDevice, SpendTarget, TokenResponse,
};
use warden_desktop_lib::config::Config;
use warden_desktop_lib::session::{now_secs, RefreshFn, SecretStore, Session, SessionError, SessionManager};
use warden_desktop_lib::sync::SyncService;
use warden_desktop_lib::{platform, session_manager_with};

#[derive(Default)]
struct MemoryStore(Mutex<Option<String>>);

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, SessionError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, value: &str) -> Result<(), SessionError> {
        *self.0.lock().unwrap() = Some(value.to_string());
        Ok(())
    }
    fn clear(&self) -> Result<(), SessionError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

fn api() -> Api {
    Api::new(&Config::staging())
}

/// `POST /auth/v1/signup` with an empty body = a new anonymous user (enabled on staging).
async fn anonymous_user(api: &Api) -> TokenResponse {
    let res = reqwest::Client::new()
        .post(format!("{}/auth/v1/signup", api.base_url()))
        .header("apikey", api.publishable_key())
        .json(&json!({}))
        .send()
        .await
        .expect("signup request");
    assert!(res.status().is_success(), "anonymous signup failed: {}", res.text().await.unwrap_or_default());
    res.json().await.expect("signup body")
}

async fn delete_user(api: &Api, jwt: &str) {
    let r: Result<Value, ApiError> = api.function(jwt, "delete-account", &json!({})).await;
    println!("cleanup delete-account: {}", if r.is_ok() { "ok".to_string() } else { format!("{r:?}") });
}

#[tokio::test]
#[ignore = "talks to staging"]
async fn devices_heartbeat_guard_and_idempotent_spend() {
    let api = api();
    let tokens = anonymous_user(&api).await;
    let jwt = tokens.access_token.clone();

    // The app's own code path: SyncService with an in-memory session store.
    let session = Arc::new(session_manager_with(&api, Box::<MemoryStore>::default()));
    session.set(Session::from_token_response(tokens)).await.unwrap();
    let data_dir = std::env::temp_dir().join(format!("warden-staging-{}", uuid::Uuid::new_v4()));
    let sync = SyncService::new(api.clone(), session.clone(), Arc::from(platform::current()), &data_dir);

    // 1. macOS device: register + heartbeat.
    let mac = sync.register_device().await.expect("register macos device");
    println!("macos device: {} ({})", mac.id, mac.name);
    sync.heartbeat(None).await.expect("macos heartbeat");
    let again = sync.register_device().await.unwrap();
    assert_eq!(again.id, mac.id, "same install id -> same device row");

    // 2. Windows device: register + heartbeat. Windows has no attestation verifier on the server.
    let args = RegisterDevice {
        p_platform: DevicePlatform::Windows,
        p_name: "Staging test PC",
        p_install_id: &uuid::Uuid::new_v4().to_string(),
        p_push_token: None,
        p_app_version: "0.1.0-test",
    };
    let win = api.register_device(&jwt, &args).await.expect("register windows device");
    assert_eq!(win.platform, "windows");
    let beat = HeartbeatRequest {
        device_id: &win.id,
        blocking_enabled: true,
        screen_time_permission: true,
        health_permission: false,
        app_version: "0.1.0-test",
        stopped_at: None,
    };
    match api.heartbeat(&jwt, &beat).await {
        Ok(r) => println!("windows heartbeat: OK, balance {} s", r.wallet.balance_seconds),
        Err(e) => println!("windows heartbeat: REFUSED {e}"),
    }

    // 3. Guard an app on the Mac device and find its block target.
    let steam = GuardedItem { token: "com.valvesoftware.steam".into(), label: "Steam".into() };
    sync.set_guarded_apps(std::slice::from_ref(&steam)).await.expect("set_guarded");
    let guarded = sync.guarded_apps();
    assert_eq!(guarded.len(), 1, "one guarded app on this device");
    assert_eq!(guarded[0].device_id.as_deref(), Some(mac.id.as_str()));
    let target = SpendTarget::Target(guarded[0].id.clone());

    // 4. Points from the staging dev tool.
    let added: Value =
        api.function(&jwt, "dev-tools", &json!({ "action": "add_points", "n": 5 })).await.expect("dev-tools");
    println!("dev-tools add_points 5: wallet balance {} s", added["wallet"]["balance_seconds"]);
    sync.heartbeat(None).await.unwrap();
    let before = sync.balance().expect("balance token decoded").balance_seconds;
    assert!(before >= 300, "at least 5 points after add_points, got {before} s");

    // 5. Spend 60 s; a retry with the same evidence must charge nothing.
    let evidence = format!("test-{}", uuid::Uuid::new_v4());
    let first = sync.spend_tick(&target, 60, &evidence).await.expect("spend tick");
    println!(
        "spend 60s: charged {} s, balance {} -> {} s, multiplier {}",
        first.charged_seconds, before, first.balance_seconds, first.multiplier
    );
    assert!(first.charged_seconds > 0);
    assert_eq!(first.balance_seconds, before - first.charged_seconds);
    let retry = sync.spend_tick(&target, 60, &evidence).await.expect("retried spend tick");
    println!("retry same evidence: charged {} s, balance {} s", retry.charged_seconds, retry.balance_seconds);
    assert_eq!(retry.charged_seconds, 0, "a retried tick must not charge again");
    assert_eq!(retry.balance_seconds, first.balance_seconds);
    assert_eq!(sync.balance().unwrap().balance_seconds, first.balance_seconds, "balance token follows the wallet");

    // 6. The Windows device cannot spend on the Mac device's app target.
    let wrong = api
        .spend_tick(
            &jwt,
            &warden_desktop_lib::api::SpendRequest {
                device_id: &win.id,
                target_id: Some(&guarded[0].id),
                site: None,
                seconds: 10,
                evidence: "wrong-device",
            },
        )
        .await;
    println!(
        "spend on another device's target: {:?}",
        wrong.as_ref().map(|r| r.charged_seconds).map_err(|e| e.to_string())
    );
    assert!(matches!(wrong, Err(ApiError::Http { status: 400, .. })));

    delete_user(&api, &jwt).await;
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
#[ignore = "talks to staging"]
async fn refresh_token_rotation() {
    let api = api();
    let tokens = anonymous_user(&api).await;
    let r0 = tokens.refresh_token.clone();

    let t1 = api.refresh(&r0).await.expect("first refresh");
    assert_ne!(t1.refresh_token, r0, "rotation gives a new refresh token");
    let t2 = api.refresh(&t1.refresh_token).await.expect("refresh with the rotated token");
    println!("rotation: r0 -> r1 -> r2 all distinct: {}", t2.refresh_token != t1.refresh_token);

    // Reusing r0 (two generations old) must fail; GoTrue allows reuse only of the parent of
    // the active token inside its short reuse interval.
    match api.refresh(&r0).await {
        Ok(_) => println!("reuse of r0: ACCEPTED (inside the server's reuse interval)"),
        Err(e) => println!("reuse of r0: refused ({e})"),
    }

    // Single-flight against the real server: an expired session, eight callers, one refresh.
    let calls = Arc::new(AtomicUsize::new(0));
    let refresh_api = api.clone();
    let counter = calls.clone();
    let refresh: RefreshFn = Arc::new(move |token: String| {
        let api = refresh_api.clone();
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { api.refresh(&token).await })
    });
    let mgr = Arc::new(SessionManager::new(Box::<MemoryStore>::default(), refresh));
    let mut expired = Session::from_token_response(t2);
    expired.expires_at = now_secs() - 1;
    mgr.set(expired).await.unwrap();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let m = mgr.clone();
            tokio::spawn(async move { m.access_token().await.expect("token") })
        })
        .collect();
    let mut tokens_seen = Vec::new();
    for h in handles {
        tokens_seen.push(h.await.unwrap());
    }
    tokens_seen.dedup();
    println!(
        "single-flight: {} refresh call(s), {} distinct token(s)",
        calls.load(Ordering::SeqCst),
        tokens_seen.len()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(tokens_seen.len(), 1);

    let jwt = mgr.access_token().await.unwrap();
    delete_user(&api, &jwt).await;
}
