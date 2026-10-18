use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::http::HeaderMap;
use base64::Engine;
use rand::RngCore;

pub const SESSION_COOKIE: &str = "llibrary_session";

#[derive(Clone)]
pub struct Auth {
    user: Arc<String>,
    password_hash: Arc<String>,
    sessions: Arc<Mutex<HashMap<String, Instant>>>,
    ttl: Duration,
}

impl Auth {
    pub fn new(user: impl Into<String>, password_hash: impl Into<String>) -> Self {
        Self {
            user: Arc::new(user.into()),
            password_hash: Arc::new(password_hash.into()),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ttl: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn verify_password(&self, user: &str, password: &str) -> bool {
        if user != self.user.as_str() {
            return false;
        }
        let Ok(parsed) = PasswordHash::new(&self.password_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    pub fn login(&self, user: &str, password: &str) -> Option<String> {
        if !self.verify_password(user, password) {
            return None;
        }
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(token.clone(), Instant::now() + self.ttl);
        }
        Some(token)
    }

    pub fn check_session(&self, token: &str) -> bool {
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let now = Instant::now();
        sessions.retain(|_, expires| *expires > now);
        sessions.contains_key(token)
    }

    pub fn logout(&self, token: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(token);
        }
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> bool {
        if let Some(token) = session_token(headers) {
            if self.check_session(&token) {
                return true;
            }
        }
        if let Some((user, password)) = basic_credentials(headers) {
            return self.verify_password(&user, &password);
        }
        false
    }
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("生成密码哈希失败: {error}"))
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            return Some(value.to_string());
        }
    }
    None
}

pub fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let encoded = header.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, password) = decoded.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::AUTHORIZATION;

    fn auth() -> Auth {
        let hash = hash_password("secret").expect("hash");
        Auth::new("reader", hash)
    }

    #[test]
    fn verifies_password() {
        let auth = auth();
        assert!(auth.verify_password("reader", "secret"));
        assert!(!auth.verify_password("reader", "wrong"));
        assert!(!auth.verify_password("other", "secret"));
    }

    #[test]
    fn session_roundtrip() {
        let auth = auth();
        let token = auth.login("reader", "secret").expect("token");
        assert!(auth.check_session(&token));
        auth.logout(&token);
        assert!(!auth.check_session(&token));
    }

    #[test]
    fn basic_auth_header() {
        let auth = auth();
        let mut headers = HeaderMap::new();
        let encoded = base64::engine::general_purpose::STANDARD.encode("reader:secret");
        headers.insert(AUTHORIZATION, format!("Basic {encoded}").parse().unwrap());
        assert!(auth.authenticate(&headers));
    }

    #[test]
    fn session_cookie_is_accepted() {
        let auth = auth();
        let token = auth.login("reader", "secret").expect("token");
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("{SESSION_COOKIE}={token}").parse().unwrap(),
        );
        assert!(auth.authenticate(&headers));
    }

    #[test]
    fn unauthenticated_is_rejected() {
        let auth = auth();
        assert!(!auth.authenticate(&HeaderMap::new()));
    }
}
