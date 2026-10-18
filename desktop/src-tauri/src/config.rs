//! Which Warden backend this build talks to.
//!
//! The Supabase URL and the publishable key are public by design: every Warden client ships
//! them, and the server protects data with row-level security and per-user tokens. They are
//! compiled in so a build cannot be pointed at another server by editing a file.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Base URL of the Supabase project, e.g. `https://xyz.supabase.co`.
    pub supabase_url: String,
    /// Publishable (anon) key. Sent as the `apikey` header on every request.
    pub publishable_key: String,
    /// Warden's Google OAuth client (the "web" client the Supabase Google provider trusts). Public:
    /// it is in every sign-in URL.
    pub google_client_id: String,
    /// Web page Google returns the browser to after sign-in; it opens `warden://auth-callback`.
    /// Must be an authorized redirect URI of `google_client_id`.
    pub auth_return_url: String,
}

/// The staging backend. Prod gets its own `config/prod.json` once it is approved.
const STAGING: &str = include_str!("../config/staging.json");

impl Config {
    pub fn staging() -> Config {
        serde_json::from_str(STAGING).expect("config/staging.json is valid JSON")
    }
}

/// Version sent to the server in register_device and heartbeat, so the crew view and support
/// can tell which build a device runs.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
