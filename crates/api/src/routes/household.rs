// SPDX-License-Identifier: AGPL-3.0-or-later
//! Members and invitations.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use chrono::Utc;
use solde_services::{
    NewMemberInput, accept_invitation, create_invitation, list_invitations, list_users,
};

use crate::auth::{ApiJson, Auth, AuthVia};
use crate::client_ip::ClientIp;
use crate::dto::{
    AcceptInvitationRequest, CreateInvitationRequest, CreatedInvitationResponse, InvitationDto,
    MeResponse, UserDto,
};
use crate::error::{ApiError, ErrorBody};
use crate::password;
use crate::routes::hashing;
use crate::routes::session::signed_in;
use crate::state::AppState;

/// Members of the household (names and roles only, never accounts).
#[utoipa::path(
    get,
    path = "/api/v1/users",
    operation_id = "listUsers",
    tag = "household",
    responses((status = 200, body = Vec<UserDto>, description = "OK"), (status = 401, body = ErrorBody, description = "`unauthorized`: not signed in"))
)]
pub(crate) async fn users(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<UserDto>>, ApiError> {
    let users = list_users(state.store(), &auth.viewer).await?;
    Ok(Json(users.into_iter().map(UserDto::from).collect()))
}

/// Invitations of the household, newest first. Admins only.
#[utoipa::path(
    get,
    path = "/api/v1/invitations",
    operation_id = "listInvitations",
    tag = "household",
    responses(
        (status = 200, body = Vec<InvitationDto>, description = "OK"),
        (status = 403, body = ErrorBody, description = "Not an admin"),
    )
)]
pub(crate) async fn invitations(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<InvitationDto>>, ApiError> {
    let invitations = list_invitations(state.store(), &auth.viewer).await?;
    Ok(Json(
        invitations.into_iter().map(InvitationDto::from).collect(),
    ))
}

/// Invites someone (valid 7 days). Admins only, with a browser session (an
/// API token cannot invite). The token is returned once.
#[utoipa::path(
    post,
    path = "/api/v1/invitations",
    operation_id = "createInvitation",
    tag = "household",
    request_body = CreateInvitationRequest,
    responses(
        (status = 201, body = CreatedInvitationResponse, description = "Created"),
        (status = 403, body = ErrorBody, description = "Not an admin, or `session_required`"),
    )
)]
pub(crate) async fn invite(
    State(state): State<AppState>,
    auth: Auth,
    ApiJson(body): ApiJson<CreateInvitationRequest>,
) -> Result<(StatusCode, Json<CreatedInvitationResponse>), ApiError> {
    if !matches!(auth.via, AuthVia::Session(_)) {
        return Err(ApiError::SessionRequired);
    }
    let created =
        create_invitation(state.store(), &auth.viewer, body.role.into(), Utc::now()).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedInvitationResponse {
            invitation: created.invitation.into(),
            token: created.token.expose().to_owned(),
        }),
    ))
}

/// Joins the household with an invitation token, then signs in. Public;
/// shares the login rate limit (5 attempts per minute and client IP).
#[utoipa::path(
    post,
    path = "/api/v1/invitations/accept",
    operation_id = "acceptInvitation",
    tag = "household",
    security(()),
    request_body = AcceptInvitationRequest,
    responses(
        (status = 201, body = MeResponse, description = "Joined; sets the session cookie"),
        (status = 404, body = ErrorBody, description = "Unknown, expired or used token"),
        (status = 409, body = ErrorBody, description = "`email_taken`"),
        (status = 422, body = ErrorBody, description = "Invalid input (see `code`)"),
        (status = 429, body = ErrorBody, description = "`rate_limited`; see `Retry-After`"),
    )
)]
pub(crate) async fn accept(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<AcceptInvitationRequest>,
) -> Result<Response, ApiError> {
    // Each attempt costs a password hash: bound them like logins.
    state
        .login_limiter
        .check(ip)
        .map_err(ApiError::RateLimited)?;
    password::validate(&body.password)?;
    let password = body.password;
    let password_hash = hashing(&state, move || password::hash(&password)).await??;
    let user = accept_invitation(
        state.store(),
        &body.token,
        NewMemberInput {
            name: body.name,
            email: body.email,
            password_hash,
        },
        Utc::now(),
    )
    .await?;
    signed_in(&state, user.id, StatusCode::CREATED).await
}
