// SPDX-License-Identifier: AGPL-3.0-or-later
//! Secrets boundary: `MasterKey`, `Vault` (seal/unseal), OS keyring (desktop
//! feature). Planned.
//!
//! Only `solde-sync` may depend on this crate. Application binaries reach it
//! only through `solde-sync`.
