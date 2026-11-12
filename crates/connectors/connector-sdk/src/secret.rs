// SPDX-License-Identifier: AGPL-3.0-or-later
//! Secret holders. None of these types implement `Debug`, `Display`,
//! `Serialize` or `Clone`; their memory is zeroized on drop. Reading the value
//! always goes through an explicit `expose*` call, which keeps every use
//! greppable.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

use crate::connector::ConnectorError;

/// A secret string (password, OTP, token, cookie value).
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Reads the secret. Never log, format or store the result.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

/// The credentials of one bank connection, keyed by
/// [`crate::CredentialField::name`]. Consumed by value by
/// [`crate::BankConnector::login`] and wiped when dropped.
#[derive(Default)]
pub struct CredentialSet {
    fields: BTreeMap<String, SecretString>,
}

impl CredentialSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style [`CredentialSet::insert`].
    pub fn with(mut self, field: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert(field, value);
        self
    }

    pub fn insert(&mut self, field: impl Into<String>, value: impl Into<String>) {
        self.fields
            .insert(field.into(), SecretString::new(value.into()));
    }

    /// Reads one credential. Never log, format or store the result.
    pub fn expose(&self, field: &str) -> Option<&str> {
        self.fields.get(field).map(SecretString::expose)
    }

    /// Like [`CredentialSet::expose`], but a missing field is an
    /// [`ConnectorError::AuthFailed`].
    pub fn expose_required(&self, field: &str) -> Result<&str, ConnectorError> {
        self.expose(field).ok_or(ConnectorError::AuthFailed)
    }

    /// Names of the fields present (not secret).
    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields.keys().map(String::as_str)
    }
}

/// Opaque session state (cookies, tokens) a connector returns from `login` and
/// gets back on the next sync. The sync engine seals it in the vault.
///
/// The content format belongs to the connector (JSON is a good choice).
pub struct SessionBlob(Zeroizing<Vec<u8>>);

impl SessionBlob {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Reads the session content. Never log, format or store the result.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// Raw bytes for the vault to seal (used by the sync engine only).
    pub fn into_sealable_bytes(self) -> Zeroizing<Vec<u8>> {
        self.0
    }

    /// Rebuilds a blob from bytes the vault has just unsealed.
    pub fn from_unsealed_bytes(bytes: Vec<u8>) -> Self {
        Self::new(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_set_access() {
        let creds = CredentialSet::new()
            .with("username", "jane")
            .with("password", "hunter2");
        assert_eq!(creds.expose("username"), Some("jane"));
        assert_eq!(creds.expose_required("password"), Ok("hunter2"));
        assert_eq!(
            creds.expose_required("otp"),
            Err(ConnectorError::AuthFailed)
        );
        assert_eq!(
            creds.field_names().collect::<Vec<_>>(),
            ["password", "username"]
        );
    }

    #[test]
    fn session_blob_round_trip() {
        let blob = SessionBlob::new(b"{\"token\":\"t\"}".to_vec());
        let bytes = blob.into_sealable_bytes();
        let back = SessionBlob::from_unsealed_bytes(bytes.to_vec());
        assert_eq!(back.expose(), b"{\"token\":\"t\"}");
    }

    #[test]
    fn secret_string_deserializes() {
        let s: SecretString = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(s.expose(), "abc");
    }
}
