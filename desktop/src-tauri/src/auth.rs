//! Sign-in: Google through the system browser (OAuth + PKCE), or a 6-digit email code.
//!
//! Google flow:
//! 1. We make a random `verifier` and send only its SHA-256 (`challenge`) in the authorize URL.
//! 2. The user signs in in their own browser (we never see the password).
//! 3. Supabase redirects the browser to Warden's web page (`auth_return_url`, e.g.
//!    `https://getwarden.app/auth?code=...`). That page shows "You're signed in" and opens
//!    `warden://auth-callback?code=...`; the OS hands that URL to us. (Redirecting straight to
//!    `warden://` worked too, but left the browser tab dimmed on Google's page forever.)
//! 4. We trade `code` + `verifier` for a session. A code intercepted by another app is useless
//!    without the verifier, which never left this process.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use url::Url;

/// A browser sign-in older than this is abandoned; its verifier is dropped.
const PENDING_TTL: Duration = Duration::from_secs(10 * 60);

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Pkce {
        // 32 random bytes -> 43 base64url chars, inside RFC 7636's 43..128 range.
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        Pkce::from_verifier(URL_SAFE_NO_PAD.encode(bytes))
    }

    fn from_verifier(verifier: String) -> Pkce {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Pkce { verifier, challenge }
    }
}

/// `GET /auth/v1/authorize` for a provider, opened in the user's browser.
///
/// `select_account`: add `prompt=select_account`. Supabase passes extra query parameters on to
/// the provider (what supabase-js `signInWithOAuth({ options: { queryParams } })` does), so
/// Google shows its account chooser instead of reusing the account the browser is signed in to.
/// Used by "Use another account" after a sign-in with an account that has no Warden.
/// `return_url` must be on the Supabase project's redirect allow-list (`uri_allow_list`).
pub fn authorize_url(
    supabase_url: &str,
    return_url: &str,
    provider: &str,
    challenge: &str,
    select_account: bool,
) -> Url {
    let mut url = Url::parse(&format!("{supabase_url}/auth/v1/authorize")).expect("valid Supabase URL");
    url.query_pairs_mut()
        .append_pair("provider", provider)
        .append_pair("redirect_to", return_url)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "s256");
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
    #[error("the sign-in link has no code")]
    NoCode,
}

/// Pull the one-time code out of `warden://auth-callback?code=...`. Errors from the provider
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
    get("code").filter(|c| !c.is_empty()).ok_or(CallbackError::NoCode)
}

/// The verifier of the browser sign-in in progress, if any. Starting again replaces it, so
/// only the latest browser tab can finish.
#[derive(Default)]
pub struct PendingSignIn(Mutex<PendingState>);

#[derive(Default)]
struct PendingState {
    verifier: Option<(String, Instant)>,
    /// The code that used the last verifier. The web page's "Open Warden" button sends the same
    /// link again; we recognise it and only bring Warden forward instead of showing an error.
    used_code: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// A new code: trade it with this verifier.
    Fresh(String),
    /// The code we already traded (or are trading right now).
    Repeat,
    /// No sign-in is waiting, or it is older than [`PENDING_TTL`].
    Expired,
}

impl PendingSignIn {
    pub fn start(&self) -> Pkce {
        let pkce = Pkce::generate();
        self.0.lock().unwrap().verifier = Some((pkce.verifier.clone(), Instant::now()));
        pkce
    }

    /// Take the verifier once for `code`; a replayed callback never gets it again.
    pub fn claim(&self, code: &str) -> Claim {
        let mut state = self.0.lock().unwrap();
        if state.used_code.as_deref() == Some(code) {
            return Claim::Repeat;
        }
        match state.verifier.take() {
            Some((verifier, started)) if started.elapsed() < PENDING_TTL => {
                state.used_code = Some(code.to_string());
                Claim::Fresh(verifier)
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
    fn challenge_matches_rfc7636_example() {
        let p = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(p.challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(Pkce::generate().verifier.len(), 43);
    }

    #[test]
    fn authorize_url_carries_redirect_and_challenge() {
        let url = authorize_url("https://x.supabase.co", "https://getwarden.app/auth", "google", "abc", false);
        let q: Vec<_> = url.query_pairs().into_owned().collect();
        assert!(q.contains(&("redirect_to".into(), "https://getwarden.app/auth".into())));
        assert!(q.contains(&("code_challenge_method".into(), "s256".into())));
        assert!(!q.iter().any(|(k, _)| k == "prompt"));
        assert_eq!(url.path(), "/auth/v1/authorize");
        let chooser = authorize_url("https://x.supabase.co", "https://getwarden.app/auth", "google", "abc", true);
        assert!(chooser.query_pairs().any(|(k, v)| k == "prompt" && v == "select_account"));
    }

    #[test]
    fn callback_parsing() {
        assert_eq!(parse_callback("warden://auth-callback?code=abc-123"), Ok("abc-123".into()));
        assert_eq!(parse_callback("warden://other?code=abc"), Err(CallbackError::NotOurs));
        assert_eq!(parse_callback("https://auth-callback?code=abc"), Err(CallbackError::NotOurs));
        assert_eq!(
            parse_callback("warden://auth-callback#error=access_denied&error_description=User+cancelled"),
            Err(CallbackError::Denied("User cancelled".into()))
        );
        assert_eq!(parse_callback("warden://auth-callback?code="), Err(CallbackError::NoCode));
    }

    #[test]
    fn pending_verifier_is_single_use() {
        let pending = PendingSignIn::default();
        assert_eq!(pending.claim("c1"), Claim::Expired);
        let pkce = pending.start();
        assert_eq!(pending.claim("c1"), Claim::Fresh(pkce.verifier));
        // "Open Warden" on the web page sends the same code again
        assert_eq!(pending.claim("c1"), Claim::Repeat);
        // another code never gets the used verifier
        assert_eq!(pending.claim("c2"), Claim::Expired);
        let next = pending.start();
        assert_eq!(pending.claim("c2"), Claim::Fresh(next.verifier));
    }

    #[test]
    fn email_check() {
        assert!(looks_like_email(" sam@gmail.com "));
        assert!(!looks_like_email("sam@gmail"));
        assert!(!looks_like_email("@gmail.com"));
        assert!(!looks_like_email("sam gmail.com"));
    }
}
