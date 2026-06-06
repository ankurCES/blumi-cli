//! Optional web auth: an argon2 password + a stateless HMAC-signed session
//! cookie. When auth is enabled, every `/api/*` route (except health, config,
//! and login) requires a valid cookie; static assets stay open so the login page
//! can load. CSRF is covered by the `SameSite=Strict` cookie.

use crate::AppState;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;
const COOKIE_NAME: &str = "blumi_session";
/// Session lifetime: 7 days.
const TTL_SECS: u64 = 7 * 24 * 3600;

/// Holds the password hash + the cookie-signing key.
pub struct Auth {
    password_hash: String,
    key: Vec<u8>,
}

impl Auth {
    pub fn new(password_hash: String, key: Vec<u8>) -> Self {
        Auth { password_hash, key }
    }

    /// Hash a password for storage (argon2id PHC string).
    pub fn hash_password(password: &str) -> anyhow::Result<String> {
        use argon2::password_hash::{rand_core::OsRng, SaltString};
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("hash failed: {e}"))
    }

    fn verify_password(&self, password: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(&self.password_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    /// Issue a signed token valid for [`TTL_SECS`]. Public so the gateway can mint
    /// a pairing token for the blugo app.
    pub fn issue(&self) -> String {
        let exp = now() + TTL_SECS;
        format!("{exp}.{}", self.sign(&exp.to_string()))
    }

    /// True if `token` is a well-formed, unexpired, correctly-signed token.
    fn verify_token(&self, token: &str) -> bool {
        let Some((exp_str, sig_b64)) = token.split_once('.') else {
            return false;
        };
        let Ok(exp) = exp_str.parse::<u64>() else {
            return false;
        };
        if now() > exp {
            return false;
        }
        let Ok(sig) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sig_b64) else {
            return false;
        };
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac key");
        mac.update(exp_str.as_bytes());
        mac.verify_slice(&sig).is_ok() // constant-time
    }

    fn sign(&self, msg: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("hmac key");
        mac.update(msg.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub password: String,
}

/// POST /api/login — verify the password, set a session cookie.
pub async fn login(State(state): State<AppState>, Json(body): Json<LoginBody>) -> Response {
    let Some(auth) = state.auth() else {
        return Json(json!({ "ok": true, "auth": false })).into_response();
    };
    if auth.verify_password(&body.password) {
        // Same token in the cookie (browser) and the body (native apps like blugo,
        // which send it back as `Authorization: Bearer <token>`).
        let token = auth.issue();
        let cookie =
            format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={TTL_SECS}");
        (
            [(header::SET_COOKIE, cookie)],
            Json(json!({ "ok": true, "token": token })),
        )
            .into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "ok": false }))).into_response()
    }
}

/// POST /api/logout — clear the session cookie.
pub async fn logout() -> Response {
    let cookie = format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response()
}

/// Middleware: gate `/api/*` (except health/config/login) behind a valid cookie.
pub async fn require_auth(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(auth) = state.auth() else {
        return next.run(req).await; // auth disabled
    };
    let path = req.uri().path();
    let exempt = !path.starts_with("/api/")
        || path == "/api/login"
        || path == "/api/health"
        || path == "/api/config"
        // Peer→peer grid endpoints authenticate with the shared grid secret
        // (checked inside the handler), not the human password/token.
        || path == "/api/grid/run"
        || path == "/api/grid/node"
        || path == "/api/grid/memory"
        || path == "/api/grid/embed";
    if exempt {
        return next.run(req).await;
    }
    // Accept the token from the cookie (browser), an `Authorization: Bearer`
    // header (native apps), or a `?token=` query param (for the SSE GET, where
    // some clients can't set headers).
    let token = req
        .headers()
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(cookie_value)
        .or_else(|| {
            req.headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(bearer_value)
        })
        .or_else(|| req.uri().query().and_then(query_token));
    let authed = token.map(|t| auth.verify_token(&t)).unwrap_or(false);
    if authed {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "authentication required").into_response()
    }
}

/// Extract the `blumi_session` value from a Cookie header.
fn cookie_value(header: &str) -> Option<String> {
    header.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == COOKIE_NAME).then(|| v.to_string())
    })
}

/// Extract the token from an `Authorization: Bearer <token>` header.
fn bearer_value(header: &str) -> Option<String> {
    header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Extract `token=<value>` from a URL query string.
fn query_token(query: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == "token").then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Auth {
        let hash = Auth::hash_password("hunter2").unwrap();
        Auth::new(hash, b"0123456789abcdef0123456789abcdef".to_vec())
    }

    #[test]
    fn password_roundtrip() {
        let a = auth();
        assert!(a.verify_password("hunter2"));
        assert!(!a.verify_password("wrong"));
    }

    #[test]
    fn token_roundtrip_and_tamper() {
        let a = auth();
        let token = a.issue();
        assert!(a.verify_token(&token));
        // Tampered signature fails.
        assert!(!a.verify_token(&format!("{}x", token)));
        // A different key rejects the token.
        let other = Auth::new(
            a.password_hash.clone(),
            b"ffffffffffffffffffffffffffffffff".to_vec(),
        );
        assert!(!other.verify_token(&token));
    }

    #[test]
    fn expired_token_fails() {
        let a = auth();
        // Hand-craft an already-expired token.
        let exp = (now() - 10).to_string();
        let forged = format!("{exp}.{}", a.sign(&exp));
        assert!(!a.verify_token(&forged));
    }

    #[test]
    fn parses_cookie_value() {
        assert_eq!(
            cookie_value("foo=1; blumi_session=abc.def; bar=2").as_deref(),
            Some("abc.def")
        );
        assert_eq!(cookie_value("other=1").as_deref(), None);
    }

    #[test]
    fn parses_bearer_and_query_token() {
        assert_eq!(bearer_value("Bearer abc.def").as_deref(), Some("abc.def"));
        assert_eq!(bearer_value("bearer xyz").as_deref(), Some("xyz"));
        assert_eq!(bearer_value("Basic abc").as_deref(), None);
        assert_eq!(bearer_value("Bearer ").as_deref(), None);
        assert_eq!(
            query_token("a=1&token=tok.sig&b=2").as_deref(),
            Some("tok.sig")
        );
        assert_eq!(query_token("a=1&b=2").as_deref(), None);
    }

    #[test]
    fn bearer_token_verifies_like_a_cookie() {
        let a = auth();
        let token = a.issue();
        // What require_auth extracts from a Bearer header must verify.
        let extracted = bearer_value(&format!("Bearer {token}")).unwrap();
        assert!(a.verify_token(&extracted));
    }
}
