//! Cookie sessions. The cookie holds a random token; the database only stores its SHA-256.

use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, HeaderMap, HeaderValue},
};
use neko_shared::User;

use crate::{crypto, db, error::AppError, AppState};

pub const COOKIE_NAME: &str = "neko_session";
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 3600;

/// The signed-in user. Use as a handler argument to require authentication.
pub struct AuthUser {
    pub user: User,
    pub session_id: i64,
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE_NAME)
        .map(|(_, v)| v.to_string())
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_TTL_SECS}{secure}"
    ))
    .expect("cookie is ASCII")
}

pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("neko_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = session_token(&parts.headers).ok_or_else(AppError::unauthorized)?;
        let hash = crypto::sha256_hex(&token);
        let found = state.db.call(move |c| db::session_user(c, &hash)).await?;
        let (session_id, user) = found.ok_or_else(AppError::unauthorized)?;
        Ok(AuthUser { user, session_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cookie() {
        let tok = "a".repeat(64);
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(&format!("x=1; {COOKIE_NAME}={tok}; y=2")).unwrap());
        assert_eq!(session_token(&h), Some(tok));
        h.insert(header::COOKIE, HeaderValue::from_static("neko_session=bad"));
        assert_eq!(session_token(&h), None);
    }
}
