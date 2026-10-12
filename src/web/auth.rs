//! Admin authentication.
//!
//! Browsers log in with the admin token once and get a random session ID in an
//! `HttpOnly; SameSite=Strict` cookie. Scripts can instead send the token as
//! `Authorization: Bearer <token>` to the JSON API. Sessions live in memory, so
//! restarting the server logs everyone out.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    extract::{Form, FromRequestParts, State},
    http::{header, request::Parts, HeaderMap, HeaderValue, Method, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::AppState;
use crate::{error::AppError, render};

pub const SESSION_COOKIE: &str = "lightpost_session";
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Slows down token guessing without needing per-client bookkeeping.
const FAILED_LOGIN_DELAY: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct Sessions {
    expires_at: Mutex<HashMap<String, Instant>>,
}

impl Sessions {
    fn create(&self) -> Result<String, AppError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|error| AppError::Internal(format!("no system randomness: {error}")))?;
        let id: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let now = Instant::now();
        let mut sessions = self
            .expires_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        sessions.retain(|_, expires_at| *expires_at > now);
        sessions.insert(id.clone(), now + SESSION_TTL);
        Ok(id)
    }

    fn is_valid(&self, id: &str) -> bool {
        let sessions = self
            .expires_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        sessions
            .get(id)
            .is_some_and(|expires_at| *expires_at > Instant::now())
    }

    fn remove(&self, id: &str) {
        self.expires_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }
}

/// Compares secrets without exiting early on the first differing byte.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
    }
    difference == 0
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Rejects cookie-authenticated writes that a browser sent from another site.
///
/// The `SameSite=Strict` cookie already stops most of these; checking
/// `Sec-Fetch-Site` and `Origin` also covers older browsers and sibling subdomains.
fn is_same_origin(headers: &HeaderMap, base_url: &str) -> bool {
    if let Some(site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        if site != "same-origin" && site != "none" {
            return false;
        }
    }
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    if origin == base_url {
        return true;
    }
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    let origin_host = origin.split_once("://").map(|(_, rest)| rest);
    host.is_some() && origin_host == host
}

fn authenticate(parts: &Parts, state: &AppState) -> Result<(), AppError> {
    if let Some(token) = bearer_token(&parts.headers) {
        return if constant_time_eq(token.as_bytes(), state.config.admin_token.as_bytes()) {
            Ok(())
        } else {
            Err(AppError::Unauthorized)
        };
    }
    let session = cookie(&parts.headers, SESSION_COOKIE).ok_or(AppError::Unauthorized)?;
    if !state.sessions.is_valid(session) {
        return Err(AppError::Unauthorized);
    }
    let safe_method = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe_method && !is_same_origin(&parts.headers, &state.config.base_url) {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Proof that a JSON API request is authenticated; fails with 401 or 403.
pub struct AdminApi;

impl FromRequestParts<AppState> for AdminApi {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        authenticate(parts, state).map(|()| Self)
    }
}

/// Proof that an admin page request is authenticated; sends visitors to the login page.
pub struct AdminPage;

impl FromRequestParts<AppState> for AdminPage {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match authenticate(parts, state) {
            Ok(()) => Ok(Self),
            Err(AppError::Unauthorized) => Err(Redirect::to("/admin/login").into_response()),
            Err(error) => Err(error.into_response()),
        }
    }
}

#[derive(Deserialize)]
pub struct LoginForm {
    #[serde(default)]
    token: String,
}

pub async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let logged_in = cookie(&headers, SESSION_COOKIE).is_some_and(|id| state.sessions.is_valid(id));
    if logged_in {
        return Redirect::to("/admin").into_response();
    }
    Html(render::admin::login(&state.config, None)).into_response()
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    if !is_same_origin(&headers, &state.config.base_url) {
        return Err(AppError::Forbidden);
    }
    if !constant_time_eq(form.token.as_bytes(), state.config.admin_token.as_bytes()) {
        tokio::time::sleep(FAILED_LOGIN_DELAY).await;
        let page = render::admin::login(&state.config, Some("토큰이 올바르지 않습니다."));
        return Ok((StatusCode::UNAUTHORIZED, Html(page)).into_response());
    }
    let session = state.sessions.create()?;
    let cookie = session_cookie(
        &session,
        SESSION_TTL.as_secs(),
        state.config.secure_cookies(),
    );
    Ok(([(header::SET_COOKIE, cookie)], Redirect::to("/admin")).into_response())
}

pub async fn logout(_: AdminPage, State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = cookie(&headers, SESSION_COOKIE) {
        state.sessions.remove(session);
    }
    let cookie = session_cookie("", 0, state.config.secure_cookies());
    ([(header::SET_COOKIE, cookie)], Redirect::to("/")).into_response()
}

fn session_cookie(value: &str, max_age: u64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    ))
    .expect("session cookie is a valid header value")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_compares_whole_values() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(!constant_time_eq(b"", b"x"));
    }

    #[test]
    fn reads_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; lightpost_session=abc; b=2"),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE), Some("abc"));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn origin_check_accepts_only_this_site() {
        let base = "http://127.0.0.1:3000";
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("blog.example"));
        assert!(is_same_origin(&headers, base));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://blog.example"),
        );
        assert!(is_same_origin(&headers, base));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!is_same_origin(&headers, base));
        headers.remove(header::ORIGIN);
        headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(!is_same_origin(&headers, base));
    }

    #[test]
    fn sessions_are_random_and_revocable() {
        let sessions = Sessions::default();
        let first = sessions.create().unwrap();
        let second = sessions.create().unwrap();
        assert_ne!(first, second);
        assert_eq!(first.len(), 64);
        assert!(sessions.is_valid(&first));
        sessions.remove(&first);
        assert!(!sessions.is_valid(&first));
        assert!(sessions.is_valid(&second));
    }
}
