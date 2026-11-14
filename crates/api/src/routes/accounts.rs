// SPDX-License-Identifier: AGPL-3.0-or-later
//! Manual accounts, sharing and manual transactions.
//!
//! An account the caller cannot see answers 404, exactly like one that does
//! not exist; a visible account without the needed permission answers 403.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use solde_core_domain::{AccountId, Currency, Date, Money, TransactionStatus, UserId};
use solde_services::{
    NewManualAccount, NewManualTransaction, create_account, create_transaction, delete_account,
    get_account, list_account_access, list_accounts, list_transactions, revoke_access,
    share_account,
};

use crate::auth::{ApiJson, Auth};
use crate::dto::{
    AccessDto, AccountDto, CreateAccountRequest, CreateTransactionRequest, GrantAccessRequest,
    ListTransactionsQuery, TransactionDto,
};
use crate::error::{ApiError, ErrorBody};
use crate::routes::parse_id;
use crate::state::AppState;

/// Transactions returned when `limit` is omitted.
const DEFAULT_TRANSACTIONS_PAGE: u32 = 100;

fn currency(code: &str) -> Result<Currency, ApiError> {
    Currency::new(code.trim()).map_err(|_| ApiError::InvalidCurrency)
}

fn date(text: &str) -> Result<Date, ApiError> {
    text.trim().parse().map_err(|_| ApiError::InvalidDate)
}

/// Accounts the caller may see, with their permission on each.
#[utoipa::path(
    get,
    path = "/api/v1/accounts",
    operation_id = "listAccounts",
    tag = "accounts",
    responses((status = 200, body = Vec<AccountDto>, description = "OK"), (status = 401, body = ErrorBody, description = "`unauthorized`: not signed in"))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<AccountDto>>, ApiError> {
    let accounts = list_accounts(state.store(), &auth.viewer).await?;
    Ok(Json(accounts.into_iter().map(AccountDto::from).collect()))
}

/// Creates a manual account owned by the caller.
#[utoipa::path(
    post,
    path = "/api/v1/accounts",
    operation_id = "createAccount",
    tag = "accounts",
    request_body = CreateAccountRequest,
    responses(
        (status = 201, body = AccountDto, description = "Created"),
        (status = 409, body = ErrorBody, description = "`duplicate_iban`"),
        (status = 422, body = ErrorBody, description = "Invalid input (see `code`)"),
    )
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    ApiJson(body): ApiJson<CreateAccountRequest>,
) -> Result<(StatusCode, Json<AccountDto>), ApiError> {
    let account = create_account(
        state.store(),
        &auth.viewer,
        NewManualAccount {
            kind: body.kind.into(),
            name: body.name,
            iban: body.iban,
            currency: currency(&body.currency)?,
            balance_minor: body.balance_minor,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(account.into())))
}

/// One account.
#[utoipa::path(
    get,
    path = "/api/v1/accounts/{account_id}",
    operation_id = "getAccount",
    tag = "accounts",
    params(("account_id" = String, Path, format = Uuid)),
    responses((status = 200, body = AccountDto, description = "OK"), (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"))
)]
pub(crate) async fn get(
    State(state): State<AppState>,
    auth: Auth,
    Path(account_id): Path<String>,
) -> Result<Json<AccountDto>, ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    let account = get_account(state.store(), &auth.viewer, account_id).await?;
    Ok(Json(account.into()))
}

/// Deletes an account with its grants and transactions. Owners only.
#[utoipa::path(
    delete,
    path = "/api/v1/accounts/{account_id}",
    operation_id = "deleteAccount",
    tag = "accounts",
    params(("account_id" = String, Path, format = Uuid)),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, body = ErrorBody, description = "Not allowed"),
        (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"),
    )
)]
pub(crate) async fn delete(
    State(state): State<AppState>,
    auth: Auth,
    Path(account_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    delete_account(state.store(), &auth.viewer, account_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Who holds which permission on the account. Owners only.
#[utoipa::path(
    get,
    path = "/api/v1/accounts/{account_id}/access",
    operation_id = "listAccountAccess",
    tag = "sharing",
    params(("account_id" = String, Path, format = Uuid)),
    responses(
        (status = 200, body = Vec<AccessDto>, description = "OK"),
        (status = 403, body = ErrorBody, description = "Not allowed"),
        (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"),
    )
)]
pub(crate) async fn access(
    State(state): State<AppState>,
    auth: Auth,
    Path(account_id): Path<String>,
) -> Result<Json<Vec<AccessDto>>, ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    let grants = list_account_access(state.store(), &auth.viewer, account_id).await?;
    Ok(Json(grants.into_iter().map(AccessDto::from).collect()))
}

/// Gives a member exactly this permission on the account. Owners only.
#[utoipa::path(
    put,
    path = "/api/v1/accounts/{account_id}/access/{user_id}",
    operation_id = "grantAccountAccess",
    tag = "sharing",
    params(
        ("account_id" = String, Path, format = Uuid),
        ("user_id" = String, Path, format = Uuid),
    ),
    request_body = GrantAccessRequest,
    responses(
        (status = 204, description = "Granted"),
        (status = 403, body = ErrorBody, description = "Not allowed"),
        (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"),
        (status = 409, body = ErrorBody, description = "`last_owner`"),
    )
)]
pub(crate) async fn grant(
    State(state): State<AppState>,
    auth: Auth,
    Path((account_id, user_id)): Path<(String, String)>,
    ApiJson(body): ApiJson<GrantAccessRequest>,
) -> Result<StatusCode, ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    let user_id: UserId = parse_id(&user_id)?;
    share_account(
        state.store(),
        &auth.viewer,
        account_id,
        user_id,
        body.permission.into(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes a member's access to the account. Owners only.
#[utoipa::path(
    delete,
    path = "/api/v1/accounts/{account_id}/access/{user_id}",
    operation_id = "revokeAccountAccess",
    tag = "sharing",
    params(
        ("account_id" = String, Path, format = Uuid),
        ("user_id" = String, Path, format = Uuid),
    ),
    responses(
        (status = 204, description = "Revoked"),
        (status = 403, body = ErrorBody, description = "Not allowed"),
        (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"),
        (status = 409, body = ErrorBody, description = "`last_owner`"),
    )
)]
pub(crate) async fn revoke(
    State(state): State<AppState>,
    auth: Auth,
    Path((account_id, user_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    let user_id: UserId = parse_id(&user_id)?;
    revoke_access(state.store(), &auth.viewer, account_id, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The most recent transactions of the account, newest first.
#[utoipa::path(
    get,
    path = "/api/v1/accounts/{account_id}/transactions",
    operation_id = "listTransactions",
    tag = "transactions",
    params(("account_id" = String, Path, format = Uuid), ListTransactionsQuery),
    responses(
        (status = 200, body = Vec<TransactionDto>, description = "OK"),
        (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"),
        (status = 422, body = ErrorBody, description = "Invalid input (see `code`)"),
    )
)]
pub(crate) async fn transactions(
    State(state): State<AppState>,
    auth: Auth,
    Path(account_id): Path<String>,
    query: Result<Query<ListTransactionsQuery>, QueryRejection>,
) -> Result<Json<Vec<TransactionDto>>, ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    let Query(query) = query.map_err(|_| ApiError::InvalidQuery)?;
    let limit = query.limit.unwrap_or(DEFAULT_TRANSACTIONS_PAGE);
    let transactions = list_transactions(state.store(), &auth.viewer, account_id, limit).await?;
    Ok(Json(
        transactions.into_iter().map(TransactionDto::from).collect(),
    ))
}

/// Records a manual transaction. Needs Write or Owner.
#[utoipa::path(
    post,
    path = "/api/v1/accounts/{account_id}/transactions",
    operation_id = "createTransaction",
    tag = "transactions",
    params(("account_id" = String, Path, format = Uuid)),
    request_body = CreateTransactionRequest,
    responses(
        (status = 201, body = TransactionDto, description = "Created"),
        (status = 403, body = ErrorBody, description = "Read-only access"),
        (status = 404, body = ErrorBody, description = "Not found, or not visible to the caller"),
        (status = 422, body = ErrorBody, description = "Invalid input (see `code`)"),
    )
)]
pub(crate) async fn create_transaction_handler(
    State(state): State<AppState>,
    auth: Auth,
    Path(account_id): Path<String>,
    ApiJson(body): ApiJson<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<TransactionDto>), ApiError> {
    let account_id: AccountId = parse_id(&account_id)?;
    let input = NewManualTransaction {
        booking_date: date(&body.booking_date)?,
        value_date: body.value_date.as_deref().map(date).transpose()?,
        amount: Money::from_minor(body.amount_minor, currency(&body.currency)?),
        label: body.label,
        status: body
            .status
            .map_or(TransactionStatus::Booked, TransactionStatus::from),
    };
    let transaction = create_transaction(state.store(), &auth.viewer, account_id, input).await?;
    Ok((StatusCode::CREATED, Json(transaction.into())))
}
