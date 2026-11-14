// SPDX-License-Identifier: AGPL-3.0-or-later
//! Schema migrations and the `schema_meta` version check.
//!
//! Each backend has its own mirrored SQL files (see `overrides.rs`); this
//! runner is shared. It records every applied migration in `schema_meta`
//! and refuses to touch a database whose schema is newer than this binary
//! (no downgrades): an older Solde writing to a newer schema could corrupt
//! data.

/// One schema migration, embedded in the binary.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Strictly increasing from 1, the same number on both backends.
    pub version: i64,
    /// Short name, recorded in `schema_meta`.
    pub name: &'static str,
    /// The whole migration script.
    pub sql: &'static str,
}

/// Highest schema version this binary knows (the last migration).
pub const SCHEMA_VERSION: i64 = 1;
