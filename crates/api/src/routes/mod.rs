// SPDX-License-Identifier: AGPL-3.0-or-later
//! Handlers, one module per area.

pub(crate) mod accounts;
pub(crate) mod household;
pub(crate) mod session;
pub(crate) mod tokens;

use std::str::FromStr;

use crate::error::ApiError;
use crate::state::AppState;

/// Parses an id from the path. A malformed id names nothing, so it answers
/// 404 like an unknown one.
pub(crate) fn parse_id<T: FromStr>(text: &str) -> Result<T, ApiError> {
    text.parse()
        .map_err(|_| ApiError::Service(solde_services::ServiceError::NotFound))
}

/// Runs password hashing or verification on the blocking pool, holding one
/// of the bounded hashing slots until it finishes: 429 when all are busy.
pub(crate) async fn hashing<T, F>(state: &AppState, work: F) -> Result<T, ApiError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let slot = state.hashing_slot()?;
    tokio::task::spawn_blocking(move || {
        let result = work();
        drop(slot);
        result
    })
    .await
    .map_err(|_| ApiError::Internal)
}
