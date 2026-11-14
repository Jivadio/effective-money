// SPDX-License-Identifier: AGPL-3.0-or-later
//! Synchronization engine: orchestrator, jobs, scheduler, deduplication,
//! source switching and quotes (planned).
//!
//! Today it only hands the application binaries the master key type: only
//! this crate may depend on `solde-secrets`, so binaries load the key
//! through here. The key will move into the sync engine's vault in M3.

pub use solde_secrets::{KeyError, MASTER_KEY_LEN, MasterKey};
