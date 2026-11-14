// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared handler state.

use std::sync::Arc;
use std::time::Duration;

use solde_services::Store;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::ApiConfig;
use crate::error::ApiError;
use crate::password::{InitError, Passwords};
use crate::rate_limit::RateLimiter;

/// Login attempts allowed per client (IPv4 address or IPv6 /64) and minute.
pub(crate) const LOGIN_ATTEMPTS_PER_MINUTE: usize = 5;

/// Login attempts allowed per minute across all clients.
pub(crate) const GLOBAL_LOGIN_ATTEMPTS_PER_MINUTE: usize = 60;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<dyn Store>,
    pub(crate) config: Arc<ApiConfig>,
    pub(crate) login_limiter: Arc<RateLimiter>,
    pub(crate) passwords: Arc<Passwords>,
    /// Bounds concurrent Argon2 work (about 19 MiB and tens of
    /// milliseconds of CPU each) to twice the CPU count.
    hashing: Arc<Semaphore>,
}

impl AppState {
    pub(crate) fn new(store: Arc<dyn Store>, config: ApiConfig) -> Result<Self, InitError> {
        let cpus = std::thread::available_parallelism().map_or(1, usize::from);
        Ok(Self {
            store,
            config: Arc::new(config),
            login_limiter: Arc::new(RateLimiter::new(
                LOGIN_ATTEMPTS_PER_MINUTE,
                GLOBAL_LOGIN_ATTEMPTS_PER_MINUTE,
                Duration::from_secs(60),
            )),
            passwords: Arc::new(Passwords::new()?),
            hashing: Arc::new(Semaphore::new(2 * cpus)),
        })
    }

    pub(crate) fn store(&self) -> &dyn Store {
        &*self.store
    }

    /// A slot to hash or verify one password, or 429 when all are busy
    /// (never queues: waiting requests would pile up memory).
    pub(crate) fn hashing_slot(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        Arc::clone(&self.hashing)
            .try_acquire_owned()
            .map_err(|_| ApiError::RateLimited(Duration::from_secs(1)))
    }
}
