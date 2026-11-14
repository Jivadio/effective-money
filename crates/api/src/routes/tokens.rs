// SPDX-License-Identifier: AGPL-3.0-or-later
//! API tokens: bearer credentials for scripts and remote clients.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{TimeDelta, Utc};
use solde_services::{ApiTokenId, create_api_token, delete_api_token, list_api_tokens};

use crate::auth::{ApiJson, Auth, AuthVia};
use crate::dto::{ApiTokenDto, CreateApiTokenRequest, CreatedApiTokenResponse};
use crate::error::{ApiError, ErrorBody};
use crate::routes::parse_id;
use crate::state::AppState;

/// Longest token lifetime, in days (about ten years).
const MAX_EXPIRY_DAYS: u32 = 3650;

/// The caller's own API tokens (never their secrets).
#[utoipa::path(
    get,
    path = "/api/v1/tokens",
    operation_id = "listApiTokens",
    tag = "tokens",
    responses((status = 200, body = Vec<ApiTokenDto>, description = "OK"), (status = 401, body = ErrorBody, description = "`unauthorized`: not signed in"))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<ApiTokenDto>>, ApiError> {
    let tokens = list_api_tokens(state.store(), &auth.viewer).await?;
    Ok(Json(tokens.into_iter().map(ApiTokenDto::from).collect()))
}

/// Creates an API token. Needs a browser session (a token cannot mint
/// tokens). The secret is returned once.
#[utoipa::path(
    post,
    path = "/api/v1/tokens",
    operation_id = "createApiToken",
    tag = "tokens",
    request_body = CreateApiTokenRequest,
    responses(
        (status = 201, body = CreatedApiTokenResponse, description = "Created"),
        (status = 403, body = ErrorBody, description = "`session_required`"),
        (status = 422, body = ErrorBody, description = "Invalid input (see `code`)"),
    )
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    ApiJson(body): ApiJson<CreateApiTokenRequest>,
) -> Result<(StatusCode, Json<CreatedApiTokenResponse>), ApiError> {
    if !matches!(auth.via, AuthVia::Session(_)) {
        return Err(ApiError::SessionRequired);
    }
    let expires_at = match body.expires_in_days {
        None => None,
        Some(days @ 1..=MAX_EXPIRY_DAYS) => Some(Utc::now() + TimeDelta::days(i64::from(days))),
        Some(_) => return Err(ApiError::InvalidExpiry),
    };
    let created = create_api_token(state.store(), &auth.viewer, &body.name, expires_at).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiTokenResponse {
            token: created.token.into(),
            secret: created.secret.expose().to_owned(),
        }),
    ))
}

/// Revokes one of the caller's tokens.
#[utoipa::path(
    delete,
    path = "/api/v1/tokens/{token_id}",
    operation_id = "deleteApiToken",
    tag = "tokens",
    params(("token_id" = String, Path, format = Uuid)),
    responses((status = 204, description = "Revoked"), (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"))
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(token_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let token_id: ApiTokenId = parse_id(&token_id)?;
    delete_api_token(state.store(), &auth.viewer, token_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
