// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`ApiError`]: every failure the API answers with, as a status code and a
//! stable machine-readable `code`.
//!
//! Bodies and logs carry only the code and a fixed English message: never
//! names, emails, IBANs, labels, amounts, passwords, tokens or hashes.

use std::time::Duration;

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use solde_services::{ServiceError, ValidationError};
use utoipa::ToSchema;

/// Error body of every non-2xx answer.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Stable machine-readable code (`not_found`, `invalid_email`, ...).
    pub code: &'static str,
    /// Short English explanation, for logs and developers. Clients show a
    /// translated message chosen from `code`.
    pub message: &'static str,
}

/// Why a request failed.
#[derive(Debug)]
pub(crate) enum ApiError {
    /// A use case refused or failed.
    Service(ServiceError),
    /// No valid session cookie or bearer token.
    Unauthorized,
    /// Wrong email or password (one answer for both).
    InvalidCredentials,
    /// `POST /setup` without the right one-time setup code.
    InvalidSetupCode,
    /// Too many login attempts from this client; retry after the delay.
    RateLimited(Duration),
    /// A state-changing request must send `Content-Type: application/json`
    /// (CSRF defence for cookie sessions; also required for JSON bodies).
    JsonRequired,
    /// The body is not valid JSON.
    MalformedJson,
    /// The JSON does not match the expected shape.
    InvalidBody,
    /// A query parameter is malformed.
    InvalidQuery,
    PasswordTooShort,
    PasswordTooLong,
    InvalidCurrency,
    InvalidDate,
    InvalidExpiry,
    /// Only a browser session (not an API token) may do this.
    SessionRequired,
    /// Something broke on our side; details are logged, not returned.
    Internal,
}

impl From<ServiceError> for ApiError {
    fn from(err: ServiceError) -> Self {
        ApiError::Service(err)
    }
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, &'static str) {
        use StatusCode as S;
        match self {
            ApiError::Service(err) => match err {
                ServiceError::NotFound => (S::NOT_FOUND, "not_found", "not found"),
                ServiceError::Forbidden => (S::FORBIDDEN, "forbidden", "not allowed"),
                ServiceError::Invalid(invalid) => {
                    let (code, message) = validation(*invalid);
                    (S::UNPROCESSABLE_ENTITY, code, message)
                }
                ServiceError::AlreadyBootstrapped => (
                    S::CONFLICT,
                    "already_bootstrapped",
                    "the instance is already set up",
                ),
                ServiceError::EmailTaken => (
                    S::CONFLICT,
                    "email_taken",
                    "this email is already used by a member",
                ),
                ServiceError::DuplicateIban => (
                    S::CONFLICT,
                    "duplicate_iban",
                    "you already have an account with this IBAN",
                ),
                ServiceError::LastOwner => (
                    S::CONFLICT,
                    "last_owner",
                    "an account must keep at least one owner",
                ),
                _ => (S::INTERNAL_SERVER_ERROR, "internal", "internal error"),
            },
            ApiError::Unauthorized => (
                S::UNAUTHORIZED,
                "unauthorized",
                "sign in or send a valid API token",
            ),
            ApiError::InvalidCredentials => (
                S::UNAUTHORIZED,
                "invalid_credentials",
                "wrong email or password",
            ),
            ApiError::InvalidSetupCode => (
                S::UNAUTHORIZED,
                "invalid_setup_code",
                "wrong setup code (see the server log)",
            ),
            ApiError::RateLimited(_) => (
                S::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many attempts, try again later",
            ),
            ApiError::JsonRequired => (
                S::UNSUPPORTED_MEDIA_TYPE,
                "json_required",
                "send Content-Type: application/json",
            ),
            ApiError::MalformedJson => (S::BAD_REQUEST, "malformed_json", "body is not valid JSON"),
            ApiError::InvalidBody => (
                S::UNPROCESSABLE_ENTITY,
                "invalid_body",
                "body does not match the expected fields",
            ),
            ApiError::InvalidQuery => (
                S::UNPROCESSABLE_ENTITY,
                "invalid_query",
                "query parameters are invalid",
            ),
            ApiError::PasswordTooShort => (
                S::UNPROCESSABLE_ENTITY,
                "password_too_short",
                "password is too short",
            ),
            ApiError::PasswordTooLong => (
                S::UNPROCESSABLE_ENTITY,
                "password_too_long",
                "password is too long",
            ),
            ApiError::InvalidCurrency => (
                S::UNPROCESSABLE_ENTITY,
                "invalid_currency",
                "not an ISO 4217 currency code",
            ),
            ApiError::InvalidDate => (
                S::UNPROCESSABLE_ENTITY,
                "invalid_date",
                "dates must be YYYY-MM-DD",
            ),
            ApiError::InvalidExpiry => (
                S::UNPROCESSABLE_ENTITY,
                "invalid_expiry",
                "expiry must be between 1 and 3650 days",
            ),
            ApiError::SessionRequired => (
                S::FORBIDDEN,
                "session_required",
                "sign in with a password to do this",
            ),
            ApiError::Internal => (S::INTERNAL_SERVER_ERROR, "internal", "internal error"),
        }
    }
}

fn validation(err: ValidationError) -> (&'static str, &'static str) {
    match err {
        ValidationError::InvalidEmail => ("invalid_email", "invalid email address"),
        ValidationError::EmptyName => ("empty_name", "name is empty"),
        ValidationError::NameTooLong => ("name_too_long", "name is too long"),
        ValidationError::EmptyLabel => ("empty_label", "label is empty"),
        ValidationError::LabelTooLong => ("label_too_long", "label is too long"),
        ValidationError::CurrencyMismatch => (
            "currency_mismatch",
            "currency differs from the account's currency",
        ),
        ValidationError::DateOutOfRange => ("date_out_of_range", "date out of range"),
        ValidationError::InvalidLimit => ("invalid_limit", "page size out of range"),
        ValidationError::InvalidIban => ("invalid_iban", "not a valid IBAN"),
        ValidationError::AmountOutOfRange => (
            "amount_out_of_range",
            "amounts must be within +/-(2^53 - 1) minor units",
        ),
        _ => ("invalid_input", "invalid input"),
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if status.is_server_error() {
            // ServiceError's Display and StoreError's class never contain
            // personal data or row values.
            match &self {
                ApiError::Service(ServiceError::Storage(err)) => {
                    let class = err.class();
                    tracing::error!(
                        %code,
                        kind = class.kind,
                        driver = class.driver,
                        db_code = class.db_code.as_deref(),
                        db_kind = class.db_kind,
                        "request failed: storage error"
                    );
                }
                ApiError::Service(err) => tracing::error!(%code, error = %err, "request failed"),
                _ => tracing::error!(%code, "request failed"),
            }
        }
        let mut response = (status, Json(ErrorBody { code, message })).into_response();
        if let ApiError::RateLimited(retry) = self {
            let seconds = retry.as_secs().max(1);
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}
