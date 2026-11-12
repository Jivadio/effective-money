// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quote and FX price handling (planned).
//!
//! Must never depend (even transitively) on `solde-secrets`: quotes coming from a
//! connector are fetched by `solde-sync` and written through `solde-services`.
