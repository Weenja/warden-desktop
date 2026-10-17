//! The signed-in session: kept in the OS keychain, refreshed before it expires.
//!
//! Security rules:
//! - Tokens live in the keychain (macOS Keychain / Windows Credential Manager) and in memory.
//!   They are never written to files, logs or the UI.
//! - Refresh tokens rotate: each refresh returns a new one and the old one dies. Two parallel
//!   refreshes would race and one would fail, so refreshing is single-flight: callers queue on
//!   one lock and the first one refreshes for all of them.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::api::{ApiError, TokenResponse};

/// Refresh this long before the access token expires, so a request never leaves with a token
/// that dies in flight.
const REFRESH_MARGIN_SECS: i64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds when `access_token` expires.
    pub expires_at: i64,
    pub user_id: String,
    pub email: Option<String>,
}

impl Session {
    pub fn from_token_response(t: TokenResponse) -> Session {
        let expires_at = t.expires_at.unwrap_or_else(|| now_secs() + t.expires_in);
        Session {
            access_token: t.access_token,
            refresh_token: t.refresh_token,
            expires_at,
            user_id: t.user.id,
            email: t.user.email,
        }
    }
}

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at - now <= REFRESH_MARGIN_SECS
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("not signed in")]
    SignedOut,
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("keychain: {0}")]
    Keychain(String),
}

/// Where the session is persisted. The app uses the OS keychain; tests use memory.
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, SessionError>;
    fn save(&self, value: &str) -> Result<(), SessionError>;
    fn clear(&self) -> Result<(), SessionError>;
}

/// One keychain item per app identifier, so staging and prod builds never share a session.
pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    const ACCOUNT: &'static str = "session";

    pub fn new(service: &str) -> KeychainStore {
        KeychainStore { service: service.to_string() }
    }

    fn entry(&self) -> Result<keyring::Entry, SessionError> {
        keyring::Entry::new(&self.service, Self::ACCOUNT).map_err(|e| SessionError::Keychain(e.to_string()))
    }
}

impl SecretStore for KeychainStore {
    fn load(&self) -> Result<Option<String>, SessionError> {
        match self.entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SessionError::Keychain(e.to_string())),
        }
    }

    fn save(&self, value: &str) -> Result<(), SessionError> {
        self.entry()?.set_password(value).map_err(|e| SessionError::Keychain(e.to_string()))
    }

    fn clear(&self) -> Result<(), SessionError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(SessionError::Keychain(e.to_string())),
        }
    }
}

/// The network half of a refresh, injected so the single-flight logic can be tested offline.
pub type RefreshFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<TokenResponse, ApiError>> + Send>> + Send + Sync>;

pub struct SessionManager {
    store: Box<dyn SecretStore>,
    refresh: RefreshFn,
    current: Mutex<Option<Session>>,
}

impl SessionManager {
    pub fn new(store: Box<dyn SecretStore>, refresh: RefreshFn) -> SessionManager {
        SessionManager { store, refresh, current: Mutex::new(None) }
    }

    /// Read the saved session at startup. A broken or unreadable item counts as signed out.
    pub async fn load(&self) -> Result<bool, SessionError> {
        let saved = self.store.load()?;
        let session = saved.and_then(|s| serde_json::from_str::<Session>(&s).ok());
        let signed_in = session.is_some();
        *self.current.lock().await = session;
        Ok(signed_in)
    }

    pub async fn set(&self, session: Session) -> Result<(), SessionError> {
        let mut current = self.current.lock().await;
        self.persist(&session)?;
        *current = Some(session);
        Ok(())
    }

    pub async fn sign_out(&self) -> Result<(), SessionError> {
        let mut current = self.current.lock().await;
        *current = None;
        self.store.clear()
    }

    pub async fn is_signed_in(&self) -> bool {
        self.current.lock().await.is_some()
    }

    pub async fn email(&self) -> Option<String> {
        self.current.lock().await.as_ref().and_then(|s| s.email.clone())
    }

    /// A valid access token, refreshed first when it is about to expire.
    pub async fn access_token(&self) -> Result<String, SessionError> {
        let mut current = self.current.lock().await;
        let session = current.as_ref().ok_or(SessionError::SignedOut)?;
        if !needs_refresh(session.expires_at, now_secs()) {
            return Ok(session.access_token.clone());
        }
        self.refresh_locked(&mut current).await
    }

    /// The server rejected `stale` (401). Refresh, unless another caller already did while we
    /// waited for the lock, in which case its fresh token is returned.
    pub async fn refresh_after_rejection(&self, stale: &str) -> Result<String, SessionError> {
        let mut current = self.current.lock().await;
        let session = current.as_ref().ok_or(SessionError::SignedOut)?;
        if session.access_token != stale {
            return Ok(session.access_token.clone());
        }
        self.refresh_locked(&mut current).await
    }

    /// Runs with the session lock held, so only one refresh is ever in flight.
    async fn refresh_locked(&self, current: &mut Option<Session>) -> Result<String, SessionError> {
        let old = current.as_ref().ok_or(SessionError::SignedOut)?;
        match (self.refresh)(old.refresh_token.clone()).await {
            Ok(tokens) => {
                let mut fresh = Session::from_token_response(tokens);
                // A refresh response always carries the email, but keep the old one if not.
                if fresh.email.is_none() {
                    fresh.email = old.email.clone();
                }
                // Save before using it: the old refresh token is already dead on the server.
                self.persist(&fresh)?;
                let token = fresh.access_token.clone();
                *current = Some(fresh);
                Ok(token)
            }
            // 400/401 from GoTrue: the refresh token is revoked, reused or expired. The user
            // must sign in again. Network errors keep the session for the next try.
            Err(ApiError::Http { status: 400 | 401, .. }) => {
                *current = None;
                self.store.clear()?;
                Err(SessionError::SignedOut)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn persist(&self, session: &Session) -> Result<(), SessionError> {
        let json = serde_json::to_string(session).expect("session serializes");
        self.store.save(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AuthUser;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct MemoryStore(StdMutex<Option<String>>);

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

    fn session(access: &str, refresh: &str, expires_at: i64) -> Session {
        Session {
            access_token: access.into(),
            refresh_token: refresh.into(),
            expires_at,
            user_id: "u1".into(),
            email: Some("sam@example.com".into()),
        }
    }

    /// A fake GoTrue that counts calls and rotates: refresh token "rN" becomes "rN+1".
    fn counting_refresh(calls: Arc<AtomicUsize>) -> RefreshFn {
        Arc::new(move |old: String| {
            let calls = calls.clone();
            Box::pin(async move {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                if old == "dead" {
                    return Err(ApiError::Http {
                        status: 400,
                        code: "refresh_token_not_found".into(),
                        message: "".into(),
                    });
                }
                Ok(TokenResponse {
                    access_token: format!("a{n}"),
                    refresh_token: format!("{old}+1"),
                    expires_at: Some(now_secs() + 3600),
                    expires_in: 3600,
                    user: AuthUser { id: "u1".into(), email: None, created_at: None, identities: None },
                })
            })
        })
    }

    #[test]
    fn refresh_margin() {
        assert!(!needs_refresh(1_000, 1_000 - 121));
        assert!(needs_refresh(1_000, 1_000 - 120));
        assert!(needs_refresh(1_000, 2_000));
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_refresh_and_rotation_is_saved() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mgr = Arc::new(SessionManager::new(Box::<MemoryStore>::default(), counting_refresh(calls.clone())));
        mgr.set(session("old", "r0", now_secs() - 10)).await.unwrap();

        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let m = mgr.clone();
                tokio::spawn(async move { m.access_token().await.unwrap() })
            })
            .collect();
        for t in tasks {
            assert_eq!(t.await.unwrap(), "a1");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "exactly one refresh for eight callers");

        let saved: Session = serde_json::from_str(&mgr.store.load().unwrap().unwrap()).unwrap();
        assert_eq!(saved.refresh_token, "r0+1", "rotated refresh token is persisted");
        assert_eq!(saved.email.as_deref(), Some("sam@example.com"), "email survives a refresh");
    }

    #[tokio::test]
    async fn rejection_refreshes_once_then_reuses_the_fresh_token() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mgr = SessionManager::new(Box::<MemoryStore>::default(), counting_refresh(calls.clone()));
        mgr.set(session("a0", "r0", now_secs() + 3600)).await.unwrap();
        assert_eq!(mgr.refresh_after_rejection("a0").await.unwrap(), "a1");
        // A second caller that also saw a0 rejected must not refresh again.
        assert_eq!(mgr.refresh_after_rejection("a0").await.unwrap(), "a1");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dead_refresh_token_signs_out() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mgr = SessionManager::new(Box::<MemoryStore>::default(), counting_refresh(calls));
        mgr.set(session("a0", "dead", now_secs() - 1)).await.unwrap();
        assert!(matches!(mgr.access_token().await, Err(SessionError::SignedOut)));
        assert!(!mgr.is_signed_in().await);
        assert!(mgr.store.load().unwrap().is_none(), "keychain item removed");
    }
}
