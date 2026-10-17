//! HTTP client for the Warden API (Supabase). This is the only network peer of the app.
//!
//! Three kinds of endpoint, all on the same host:
//! - Auth (`/auth/v1/...`): sign-in and token refresh. Only the publishable key is sent.
//! - Edge functions (`/functions/v1/...`): heartbeat and spend ticks. Need the user's JWT.
//! - PostgREST (`/rest/v1/...`): RPCs and tables, guarded by row-level security on the server.
//!
//! Every field sent here is listed in DATA.md. Tokens are never logged.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;

/// A network call that takes longer than this is treated as failed. The heartbeat and the
/// drain loop retry on their own schedule, so a hung request must not block them.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The server answered with an error status. `code` is the stable machine code when the
    /// server sent one (edge functions: `error.code`, GoTrue: `error_code`, PostgREST: `code`).
    #[error("server said {status} {code}: {message}")]
    Http { status: u16, code: String, message: String },
    /// No answer: offline, DNS, TLS or timeout.
    #[error("network error: {0}")]
    Network(String),
    /// The server answered 2xx with a body we do not understand.
    #[error("unexpected response: {0}")]
    Decode(String),
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

// ---------- Auth types ----------

/// What GoTrue returns for every successful sign-in or refresh.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds since the Unix epoch when `access_token` expires.
    pub expires_at: Option<i64>,
    /// Seconds from now; used when `expires_at` is missing.
    pub expires_in: i64,
    pub user: AuthUser,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
}

// ---------- Device, wallet and block list types ----------

/// `public.devices` row returned by `register_device`. Only the fields we use.
#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: Option<String>,
    pub platform: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DevicePlatform {
    Macos,
    Windows,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterDevice<'a> {
    pub p_platform: DevicePlatform,
    pub p_name: &'a str,
    pub p_install_id: &'a str,
    /// Desktop has no push channel; the server keeps the field for phones.
    pub p_push_token: Option<&'a str>,
    pub p_app_version: &'a str,
}

/// The wallet as `warden.wallet()` reports it. Only the fields the desktop app reads.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Wallet {
    pub balance_seconds: i64,
    pub low_balance: bool,
    pub multiplier: f64,
    pub day_key: String,
    pub resets_at: Option<String>,
    /// The crew's daily max: earning stops there. The battery shows balance / daily max.
    pub daily_max_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatRequest<'a> {
    pub device_id: &'a str,
    /// Protection is on while Warden runs. A Quit sends one last heartbeat with `stopped_at`.
    pub blocking_enabled: bool,
    /// The phone's "Screen Time / Usage Access" flag. A Mac or PC needs no permission to see
    /// the frontmost app, so this is always true here; false would tell the crew it was revoked.
    pub screen_time_permission: bool,
    /// Health data lives on the phone only.
    pub health_permission: bool,
    pub app_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<&'a str>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatResponse {
    pub wallet: Wallet,
    pub balance_token: String,
    pub balance_token_expires_at: String,
}

/// What a spend tick charges: a guarded app (`target_id`) or a guarded site (`site`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpendTarget {
    Target(String),
    Site(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct SpendRequest<'a> {
    pub device_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<&'a str>,
    /// Foreground seconds since the last tick, 1..=900.
    pub seconds: u32,
    /// Idempotency key for this tick: a retry with the same key charges nothing.
    pub evidence: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpendResponse {
    pub charged_seconds: i64,
    pub balance_seconds: i64,
    pub multiplier: f64,
    pub low_balance: bool,
    pub shield: bool,
    pub wallet: Wallet,
    pub balance_token: String,
    pub balance_token_expires_at: String,
}

/// A row of `public.block_targets`. `device_id` None means "every device" (sites).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BlockTarget {
    pub id: String,
    pub kind: String,
    pub token: String,
    pub label: String,
    pub device_id: Option<String>,
}

/// One item for `set_guarded`: an app bundle id (Mac) or exe name (Windows) plus its label.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardedItem {
    pub token: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CrewState {
    #[serde(default)]
    pub members: Option<Vec<CrewMember>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CrewMember {
    pub name: String,
    #[serde(default)]
    pub me: bool,
}

// ---------- Client ----------

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    base: String,
    key: String,
}

impl Api {
    pub fn new(config: &Config) -> Api {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("WardenDesktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client builds with static settings");
        Api { http, base: config.supabase_url.trim_end_matches('/').to_string(), key: config.publishable_key.clone() }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub fn publishable_key(&self) -> &str {
        &self.key
    }

    // ----- Auth -----

    /// Swap the one-time code from the OAuth redirect for a session. The verifier proves we
    /// started this sign-in, so a code stolen from the redirect is useless to anyone else.
    pub async fn exchange_pkce(&self, auth_code: &str, verifier: &str) -> Result<TokenResponse, ApiError> {
        let body = json!({ "auth_code": auth_code, "code_verifier": verifier });
        self.auth_post("token?grant_type=pkce", &body).await
    }

    /// Trade the refresh token for a new session. Rotation is on: the old refresh token stops
    /// working, so the caller must store the new one before anything else.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, ApiError> {
        let body = json!({ "refresh_token": refresh_token });
        self.auth_post("token?grant_type=refresh_token", &body).await
    }

    /// Email a 6-digit sign-in code. `create_user: false`: accounts are made on the phone, so a
    /// typo cannot create an empty account here.
    pub async fn send_email_code(&self, email: &str) -> Result<(), ApiError> {
        let body = json!({ "email": email, "create_user": false });
        let _: Value = self.auth_post("otp", &body).await?;
        Ok(())
    }

    pub async fn verify_email_code(&self, email: &str, code: &str) -> Result<TokenResponse, ApiError> {
        let body = json!({ "type": "email", "email": email, "token": code });
        self.auth_post("verify", &body).await
    }

    async fn auth_post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T, ApiError> {
        let req = self.http.post(format!("{}/auth/v1/{path}", self.base)).header("apikey", &self.key).json(body);
        send(req).await
    }

    // ----- Devices, wallet, block list -----

    /// Create or update this computer's device row. The install id makes it idempotent: the
    /// same install always maps to the same device.
    pub async fn register_device(&self, jwt: &str, args: &RegisterDevice<'_>) -> Result<Device, ApiError> {
        self.rpc(jwt, "register_device", args).await
    }

    /// Tell the server this device is alive (the crew sees a silent device as "went dark") and
    /// get the current wallet plus a fresh balance token.
    pub async fn heartbeat(&self, jwt: &str, body: &HeartbeatRequest<'_>) -> Result<HeartbeatResponse, ApiError> {
        self.function(jwt, "heartbeat", body).await
    }

    /// Charge foreground seconds on a guarded app or site. The server is authoritative.
    pub async fn spend_tick(&self, jwt: &str, body: &SpendRequest<'_>) -> Result<SpendResponse, ApiError> {
        self.function(jwt, "spend-tick", body).await
    }

    /// The user's guarded apps and sites. RLS returns only the caller's rows.
    pub async fn block_targets(&self, jwt: &str) -> Result<Vec<BlockTarget>, ApiError> {
        let req = self
            .http
            .get(format!("{}/rest/v1/block_targets", self.base))
            .query(&[("select", "id,kind,token,label,device_id"), ("order", "created_at.asc")])
            .header("apikey", &self.key)
            .bearer_auth(jwt);
        send(req).await
    }

    /// Replace the guarded list of one kind. Apps are per device (`device` = this computer).
    pub async fn set_guarded(
        &self,
        jwt: &str,
        kind: &str,
        items: &[GuardedItem],
        device: Option<&str>,
    ) -> Result<Value, ApiError> {
        let body = json!({ "p_kind": kind, "p_items": items, "p_device": device });
        self.rpc(jwt, "set_guarded", &body).await
    }

    /// The crew (names only) so the popover can say who is told when Warden stops.
    pub async fn crew_state(&self, jwt: &str) -> Result<CrewState, ApiError> {
        self.rpc(jwt, "crew_state", &json!({})).await
    }

    async fn rpc<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        jwt: &str,
        name: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let req = self
            .http
            .post(format!("{}/rest/v1/rpc/{name}", self.base))
            .header("apikey", &self.key)
            .bearer_auth(jwt)
            .json(body);
        send(req).await
    }

    /// Call an edge function. Public so the staging tests can reach `dev-tools`.
    pub async fn function<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        jwt: &str,
        name: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let req = self
            .http
            .post(format!("{}/functions/v1/{name}", self.base))
            .header("apikey", &self.key)
            .bearer_auth(jwt)
            .json(body);
        send(req).await
    }
}

async fn send<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> Result<T, ApiError> {
    let res = req.send().await.map_err(|e| ApiError::Network(e.to_string()))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| ApiError::Network(e.to_string()))?;
    if !status.is_success() {
        return Err(parse_error(status.as_u16(), &text));
    }
    serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()))
}

/// The three backends shape errors differently; pull out a stable code and a message.
fn parse_error(status: u16, body: &str) -> ApiError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let pick = |paths: &[&[&str]]| -> Option<String> {
        paths.iter().find_map(|path| {
            let mut cur = &v;
            for key in *path {
                cur = cur.get(key)?;
            }
            match cur {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
        })
    };
    let code = pick(&[&["error", "code"], &["error_code"], &["code"], &["error"]]).unwrap_or_else(|| "unknown".into());
    let message = pick(&[&["error", "message"], &["msg"], &["message"], &["error_description"]])
        .unwrap_or_else(|| body.chars().take(200).collect());
    ApiError::Http { status, code, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_from_each_backend() {
        let edge = parse_error(429, r#"{"error":{"code":"rate_limited","message":"slow down"}}"#);
        assert!(matches!(edge, ApiError::Http { ref code, .. } if code == "rate_limited"));
        let gotrue =
            parse_error(400, r#"{"code":400,"error_code":"refresh_token_already_used","msg":"Invalid Refresh Token"}"#);
        assert!(matches!(gotrue, ApiError::Http { ref code, ref message, .. }
            if code == "refresh_token_already_used" && message == "Invalid Refresh Token"));
        let rest = parse_error(400, r#"{"code":"P0001","message":"not your device","details":null}"#);
        assert!(matches!(rest, ApiError::Http { ref code, .. } if code == "P0001"));
    }
}
