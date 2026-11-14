// SPDX-License-Identifier: AGPL-3.0-or-later
//! Who is calling: a session cookie or a bearer token, turned into a
//! [`Viewer`]. Also the CSRF rule and the session cookie format.
//!
//! # CSRF
//!
//! The session cookie is `SameSite=Strict`, so browsers do not send it on
//! cross-site requests. As a second layer, every state-changing request
//! (anything but `GET`, `HEAD` and `OPTIONS`) authenticated by the cookie
//! must carry `Content-Type: application/json`: a cross-site HTML form
//! cannot set that header, and a cross-site `fetch` that sets it needs a
//! CORS preflight, which this API never grants. Bearer-token requests are
//! not exposed to CSRF (browsers never attach the header on their own).

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, header};
use chrono::Utc;
use serde::de::DeserializeOwned;
use solde_services::{SESSION_TTL, Viewer, api_token_viewer, session_viewer};

use crate::error::ApiError;
use crate::state::AppState;

/// Name of the session cookie. It holds only a random token; the database
/// keeps its hash. The `__Host-` prefix makes browsers accept it only with
/// `Secure`, `Path=/` and no `Domain`, so another site or subdomain cannot
/// plant or overwrite it.
pub const SESSION_COOKIE: &str = "__Host-solde_session";

/// Name of the session cookie with `SOLDE_INSECURE_COOKIES` (plain HTTP):
/// browsers refuse `__Host-` cookies without `Secure`.
pub const INSECURE_SESSION_COOKIE: &str = "solde_session";

/// The session cookie's name: [`SESSION_COOKIE`] when it is `Secure`,
/// [`INSECURE_SESSION_COOKIE`] otherwise.
pub(crate) fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SESSION_COOKIE
    } else {
        INSECURE_SESSION_COOKIE
    }
}

/// How the caller authenticated.
pub(crate) enum AuthVia {
    /// Browser session; holds the cookie's token (for logout).
    Session(String),
    /// `Authorization: Bearer` API token.
    Bearer,
}

/// An authenticated caller.
pub(crate) struct Auth {
    pub(crate) viewer: Viewer,
    pub(crate) via: AuthVia,
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let now = Utc::now();
        if let Some(value) = parts.headers.get(header::AUTHORIZATION) {
            let token = value
                .to_str()
                .ok()
                .and_then(bearer_token)
                .ok_or(ApiError::Unauthorized)?;
            let viewer = api_token_viewer(state.store(), token, now)
                .await?
                .ok_or(ApiError::Unauthorized)?;
            return Ok(Auth {
                viewer,
                via: AuthVia::Bearer,
            });
        }
        let secure = !state.config.insecure_cookies;
        let token = session_token(&parts.headers, secure).ok_or(ApiError::Unauthorized)?;
        if !is_safe(&parts.method) && !is_json(&parts.headers) {
            return Err(ApiError::JsonRequired);
        }
        let viewer = session_viewer(state.store(), &token, now)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        Ok(Auth {
            viewer,
            via: AuthVia::Session(token),
        })
    }
}

fn bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

/// The session token from the `Cookie` header(s), if any. Only the cookie
/// name of the current mode counts.
fn session_token(headers: &HeaderMap, secure: bool) -> Option<String> {
    let name = cookie_name(secure);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, value)| *key == name && !value.is_empty())
        .map(|(_, value)| value.to_owned())
}

/// `Set-Cookie` value opening a session.
pub(crate) fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        cookie_name(secure),
        SESSION_TTL.num_seconds(),
        if secure { "; Secure" } else { "" }
    )
}

/// `Set-Cookie` value removing the session cookie.
pub(crate) fn clear_session_cookie(secure: bool) -> String {
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
        cookie_name(secure),
        if secure { "; Secure" } else { "" }
    )
}

/// A JSON body whose rejections are [`ApiError`]s without details: the
/// default messages can quote the input, which may hold a password.
pub(crate) struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::JsonRequired),
            Err(JsonRejection::JsonSyntaxError(_)) => Err(ApiError::MalformedJson),
            Err(JsonRejection::JsonDataError(_)) => Err(ApiError::InvalidBody),
            Err(_) => Err(ApiError::InvalidBody),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reads_bearer_and_cookie() {
        assert_eq!(bearer_token("Bearer abc"), Some("abc"));
        assert_eq!(bearer_token("bearer  abc "), Some("abc"));
        assert_eq!(bearer_token("Basic abc"), None);
        assert_eq!(bearer_token("Bearer "), None);

        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("theme=dark"));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=b; solde_session=plain; __Host-solde_session=tok123; c=d"),
        );
        assert_eq!(session_token(&headers, true).as_deref(), Some("tok123"));
        assert_eq!(session_token(&headers, false).as_deref(), Some("plain"));
        assert_eq!(session_token(&HeaderMap::new(), true), None);
    }

    #[test]
    fn json_content_type_rule() {
        let mut headers = HeaderMap::new();
        assert!(!is_json(&headers));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        assert!(is_json(&headers));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(!is_json(&headers));
    }

    #[test]
    fn cookie_flags() {
        let secure = session_cookie("tok", true);
        assert!(secure.contains("HttpOnly") && secure.contains("SameSite=Strict"));
        assert!(secure.ends_with("; Secure"));
        assert!(secure.starts_with("__Host-solde_session=tok;"));
        let plain = session_cookie("tok", false);
        assert!(!plain.contains("Secure"));
        assert!(plain.starts_with("solde_session=tok;"));
        assert!(clear_session_cookie(true).contains("Max-Age=0"));
    }
}
