// SPDX-License-Identifier: AGPL-3.0-or-later
//! Instance metadata, first-start setup, login, logout and `me`.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use solde_core_domain::UserId;
use solde_services::{
    NewMemberInput, Viewer, bootstrap, close_session, current_member, find_credentials,
    is_bootstrapped, open_session,
};

use crate::auth::{ApiJson, Auth, AuthVia, clear_session_cookie, session_cookie};
use crate::client_ip::ClientIp;
use crate::dto::{LoginRequest, MeResponse, MetaResponse, SetupRequest};
use crate::error::{ApiError, ErrorBody};
use crate::password;
use crate::routes::hashing;
use crate::state::AppState;
use crate::{API_VERSION, MIN_CLIENT_VERSION};

/// API version and whether the instance is set up. Public.
#[utoipa::path(
    get,
    path = "/api/v1/meta",
    operation_id = "getMeta",
    tag = "instance",
    security(()),
    responses((status = 200, body = MetaResponse, description = "OK"))
)]
pub(crate) async fn meta(State(state): State<AppState>) -> Result<Json<MetaResponse>, ApiError> {
    Ok(Json(MetaResponse {
        api_version: API_VERSION,
        min_client_version: MIN_CLIENT_VERSION,
        bootstrapped: is_bootstrapped(state.store()).await?,
    }))
}

/// Creates the household and its first admin, then signs them in. Works
/// once per instance, and needs the one-time setup code the server prints
/// in its log at startup. Shares the login rate limit.
#[utoipa::path(
    post,
    path = "/api/v1/setup",
    operation_id = "setup",
    tag = "instance",
    security(()),
    request_body = SetupRequest,
    responses(
        (status = 201, body = MeResponse, description = "Created; sets the session cookie"),
        (status = 401, body = ErrorBody, description = "`invalid_setup_code`"),
        (status = 409, body = ErrorBody, description = "`already_bootstrapped`"),
        (status = 422, body = ErrorBody, description = "Invalid input (see `code`)"),
        (status = 429, body = ErrorBody, description = "`rate_limited`; see `Retry-After`"),
    )
)]
pub(crate) async fn setup(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<SetupRequest>,
) -> Result<Response, ApiError> {
    state
        .login_limiter
        .check(ip)
        .map_err(ApiError::RateLimited)?;
    // Cheap early refusal; `bootstrap` itself is atomic.
    if is_bootstrapped(state.store()).await? {
        return Err(solde_services::ServiceError::AlreadyBootstrapped.into());
    }
    let code_ok = state
        .config
        .setup_code
        .as_ref()
        .is_some_and(|code| code.matches(&body.setup_code));
    if !code_ok {
        return Err(ApiError::InvalidSetupCode);
    }
    password::validate(&body.password)?;
    let password = body.password;
    let password_hash = hashing(&state, move || password::hash(&password)).await??;
    let (_, admin) = bootstrap(
        state.store(),
        &body.household_name,
        NewMemberInput {
            name: body.name,
            email: body.email,
            password_hash,
        },
    )
    .await?;
    signed_in(&state, admin.id, StatusCode::CREATED).await
}

/// Signs in with email and password; sets the session cookie. Limited to
/// 5 attempts per minute and client IP.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    operation_id = "login",
    tag = "session",
    security(()),
    request_body = LoginRequest,
    responses(
        (status = 200, body = MeResponse, description = "Signed in; sets the session cookie"),
        (status = 401, body = ErrorBody, description = "`invalid_credentials`"),
        (status = 429, body = ErrorBody, description = "`rate_limited`; see `Retry-After`"),
    )
)]
pub(crate) async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<LoginRequest>,
) -> Result<Response, ApiError> {
    state
        .login_limiter
        .check(ip)
        .map_err(ApiError::RateLimited)?;
    let credentials = find_credentials(state.store(), &body.email).await?;
    // Unknown emails verify against a dummy hash, so both cases take as
    // long and answer the same.
    let (user, hash) = match credentials {
        Some(found) => (Some(found.user), found.password_hash),
        None => (None, state.passwords.dummy_hash().to_owned()),
    };
    let password = body.password;
    let matches = hashing(&state, move || password::verify(&password, &hash)).await?;
    match user {
        Some(user) if matches => signed_in(&state, user.id, StatusCode::OK).await,
        _ => Err(ApiError::InvalidCredentials),
    }
}

/// Ends the current browser session and clears the cookie. With an API
/// token it only answers 204 (delete the token instead).
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    operation_id = "logout",
    tag = "session",
    responses((status = 204, description = "Signed out; clears the session cookie"))
)]
pub(crate) async fn logout(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Response, ApiError> {
    if let AuthVia::Session(token) = &auth.via {
        close_session(state.store(), token).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    set_cookie(
        response.headers_mut(),
        &clear_session_cookie(!state.config.insecure_cookies),
    )?;
    Ok(response)
}

/// The signed-in member and their household.
#[utoipa::path(
    get,
    path = "/api/v1/me",
    operation_id = "getMe",
    tag = "session",
    responses((status = 200, body = MeResponse, description = "OK"), (status = 401, body = ErrorBody, description = "`unauthorized`: not signed in"))
)]
pub(crate) async fn me(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<MeResponse>, ApiError> {
    Ok(Json(me_response(&state, &auth.viewer).await?))
}

pub(crate) async fn me_response(state: &AppState, viewer: &Viewer) -> Result<MeResponse, ApiError> {
    let (user, household) = current_member(state.store(), viewer).await?;
    Ok(MeResponse {
        user: user.into(),
        household: household.into(),
    })
}

/// Opens a session for `user_id` and answers `status` with the session
/// cookie and the member.
pub(crate) async fn signed_in(
    state: &AppState,
    user_id: UserId,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let opened = open_session(state.store(), user_id, Utc::now()).await?;
    let viewer = Viewer::load(state.store(), user_id).await?;
    let body = me_response(state, &viewer).await?;
    let mut response = (status, Json(body)).into_response();
    set_cookie(
        response.headers_mut(),
        &session_cookie(opened.token.expose(), !state.config.insecure_cookies),
    )?;
    Ok(response)
}

fn set_cookie(headers: &mut HeaderMap, value: &str) -> Result<(), ApiError> {
    let value = HeaderValue::from_str(value).map_err(|_| ApiError::Internal)?;
    headers.insert(header::SET_COOKIE, value);
    Ok(())
}
