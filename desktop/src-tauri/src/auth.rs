//! Sign-in: Google through the system browser (OpenID Connect ID token), or a 6-digit email code.
//!
//! Google flow:
//! 1. We make a random `nonce` and send only its SHA-256 to Google in the sign-in URL.
//! 2. The user signs in in their own browser (we never see the password). Google's page names
//!    Warden's own site, because the browser returns there, not to the Supabase project URL.
//! 3. Google redirects the browser to Warden's web page (`auth_return_url`, e.g.
//!    `https://getwarden.app/auth#id_token=...`). That page shows "You're signed in" and opens
//!    `warden://auth-callback#id_token=...`; the OS hands that URL to us. The token travels in
//!    the URL fragment, which the browser never sends to a server.
//! 4. We send the ID token and the raw nonce to Supabase (`grant_type=id_token`). Supabase checks
//!    Google's signature, that the token is for Warden's Google client, and that the SHA-256 of
//!    the nonce is the one in the token. A token intercepted by another app is useless without
//!    the nonce, which never left this process. The Android app signs in the same way.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use url::Url;

/// A browser sign-in older than this is abandoned; its nonce is dropped.
const PENDING_TTL: Duration = Duration::from_secs(10 * 60);

/// Google's OpenID Connect sign-in page (`authorization_endpoint` in
/// https://accounts.google.com/.well-known/openid-configuration).
const GOOGLE_AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";

pub struct Nonce {
    /// Stays in this process; goes to Supabase with the ID token.
    pub raw: String,
    /// Hex SHA-256 of `raw`; goes to Google, which copies it into the ID token.
    pub hashed: String,
}

impl Nonce {
    pub fn generate() -> Nonce {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        Nonce::from_raw(URL_SAFE_NO_PAD.encode(bytes))
    }

    fn from_raw(raw: String) -> Nonce {
        // Supabase compares the hex SHA-256 of the nonce it gets with the token's `nonce` claim.
        let hashed = format!("{:x}", Sha256::digest(raw.as_bytes()));
        Nonce { raw, hashed }
    }
}

/// Google's sign-in page, opened in the user's browser. `response_type=id_token`: Google sends
/// the ID token straight to `return_url` in the URL fragment; no code, so no client secret.
///
/// `select_account`: add `prompt=select_account`, so Google shows its account chooser instead of
/// reusing the account the browser is signed in to. Used by "Use another account" after a
/// sign-in with an account that has no Warden.
/// `return_url` must be an authorized redirect URI of the Google client `client_id`.
pub fn google_authorize_url(client_id: &str, return_url: &str, hashed_nonce: &str, select_account: bool) -> Url {
    let mut url = Url::parse(GOOGLE_AUTHORIZE).expect("valid Google URL");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", return_url)
        .append_pair("response_type", "id_token")
        .append_pair("scope", "openid email profile")
        .append_pair("nonce", hashed_nonce);
    if select_account {
        url.query_pairs_mut().append_pair("prompt", "select_account");
    }
    url
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CallbackError {
    #[error("not a Warden sign-in link")]
    NotOurs,
    #[error("{0}")]
    Denied(String),
    #[error("the sign-in link has no Google token")]
    NoToken,
}

/// Pull the Google ID token out of `warden://auth-callback#id_token=...`. Errors from Google
/// ("access_denied" when the user cancels) arrive in the query or the fragment.
pub fn parse_callback(raw: &str) -> Result<String, CallbackError> {
    let url = Url::parse(raw).map_err(|_| CallbackError::NotOurs)?;
    if url.scheme() != "warden" || url.host_str() != Some("auth-callback") {
        return Err(CallbackError::NotOurs);
    }
    let fragment_pairs = url
        .fragment()
        .map(|f| url::form_urlencoded::parse(f.as_bytes()).into_owned().collect::<Vec<_>>())
        .unwrap_or_default();
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().chain(fragment_pairs).collect();
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());

    if let Some(err) = get("error_description").or_else(|| get("error")) {
        return Err(CallbackError::Denied(err));
    }
    get("id_token").filter(|t| !t.is_empty()).ok_or(CallbackError::NoToken)
}

/// The nonce of the browser sign-in in progress, if any. Starting again replaces it, so only
/// the latest browser tab can finish.
#[derive(Default)]
pub struct PendingSignIn(Mutex<PendingState>);

#[derive(Default)]
struct PendingState {
    nonce: Option<(String, Instant)>,
    /// The token that used the last nonce. The web page's "Open Warden" button sends the same
    /// link again; we recognise it and only bring Warden forward instead of showing an error.
    used_token: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// A new token: send it to Supabase with this raw nonce.
    Fresh(String),
    /// The token we already sent (or are sending right now).
    Repeat,
    /// No sign-in is waiting, or it is older than [`PENDING_TTL`].
    Expired,
}

impl PendingSignIn {
    pub fn start(&self) -> Nonce {
        let nonce = Nonce::generate();
        self.0.lock().unwrap().nonce = Some((nonce.raw.clone(), Instant::now()));
        nonce
    }

    /// Take the nonce once for `id_token`; a replayed callback never gets it again.
    pub fn claim(&self, id_token: &str) -> Claim {
        let mut state = self.0.lock().unwrap();
        if state.used_token.as_deref() == Some(id_token) {
            return Claim::Repeat;
        }
        match state.nonce.take() {
            Some((raw, started)) if started.elapsed() < PENDING_TTL => {
                state.used_token = Some(id_token.to_string());
                Claim::Fresh(raw)
            }
            _ => Claim::Expired,
        }
    }
}

/// Light check before we ask the server to email a code.
pub fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty() && domain.contains('.') && !domain.starts_with('.') && !s.contains(' ')
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_hash_is_hex_sha256() {
        // echo -n abc | shasum -a 256
        assert_eq!(
            Nonce::from_raw("abc".into()).hashed,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let n = Nonce::generate();
        assert_eq!(n.raw.len(), 43);
        assert_ne!(n.raw, Nonce::generate().raw);
    }

    #[test]
    fn google_url_asks_for_an_id_token_with_the_hashed_nonce() {
        let url = google_authorize_url("cid.apps.googleusercontent.com", "https://getwarden.app/auth", "abc", false);
        let q: Vec<_> = url.query_pairs().into_owned().collect();
        let has = |k: &str, v: &str| q.contains(&(k.to_string(), v.to_string()));
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        assert!(has("client_id", "cid.apps.googleusercontent.com"));
        assert!(has("redirect_uri", "https://getwarden.app/auth"));
        assert!(has("response_type", "id_token"));
        assert!(has("scope", "openid email profile"));
        assert!(has("nonce", "abc"));
        assert!(!q.iter().any(|(k, _)| k == "prompt"));
        let chooser = google_authorize_url("cid", "https://getwarden.app/auth", "abc", true);
        assert!(chooser.query_pairs().any(|(k, v)| k == "prompt" && v == "select_account"));
    }

    #[test]
    fn callback_parsing() {
        assert_eq!(parse_callback("warden://auth-callback#id_token=eyJ.a-b.c_d"), Ok("eyJ.a-b.c_d".into()));
        assert_eq!(parse_callback("warden://auth-callback?id_token=eyJ.x.y"), Ok("eyJ.x.y".into()));
        assert_eq!(parse_callback("warden://other#id_token=eyJ"), Err(CallbackError::NotOurs));
        assert_eq!(parse_callback("https://auth-callback#id_token=eyJ"), Err(CallbackError::NotOurs));
        assert_eq!(
            parse_callback("warden://auth-callback#error=access_denied&error_description=User+cancelled"),
            Err(CallbackError::Denied("User cancelled".into()))
        );
        assert_eq!(
            parse_callback("warden://auth-callback?error=access_denied"),
            Err(CallbackError::Denied("access_denied".into()))
        );
        assert_eq!(parse_callback("warden://auth-callback#id_token="), Err(CallbackError::NoToken));
        // the old Supabase code flow is not accepted any more
        assert_eq!(parse_callback("warden://auth-callback?code=abc"), Err(CallbackError::NoToken));
    }

    #[test]
    fn pending_nonce_is_single_use() {
        let pending = PendingSignIn::default();
        assert_eq!(pending.claim("t1"), Claim::Expired);
        let nonce = pending.start();
        assert_eq!(pending.claim("t1"), Claim::Fresh(nonce.raw));
        // "Open Warden" on the web page sends the same token again
        assert_eq!(pending.claim("t1"), Claim::Repeat);
        // another token never gets the used nonce
        assert_eq!(pending.claim("t2"), Claim::Expired);
        let next = pending.start();
        assert_eq!(pending.claim("t2"), Claim::Fresh(next.raw));
    }

    #[test]
    fn email_check() {
        assert!(looks_like_email(" sam@gmail.com "));
        assert!(!looks_like_email("sam@gmail"));
        assert!(!looks_like_email("@gmail.com"));
        assert!(!looks_like_email("sam gmail.com"));
    }
}
