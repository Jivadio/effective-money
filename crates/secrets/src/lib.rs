// SPDX-License-Identifier: AGPL-3.0-or-later
//! Secrets boundary. Today: the [`MasterKey`], loaded and checked at
//! startup. Planned: `Vault` (seal/unseal of bank credentials) and the OS
//! keyring (desktop feature).
//!
//! Only `solde-sync` may depend on this crate. Application binaries reach it
//! only through `solde-sync`.

use std::fmt;
use std::io::Read as _;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroizing;

/// Key length in bytes (256 bits).
pub const MASTER_KEY_LEN: usize = 32;

/// Largest key file accepted, in bytes: a base64 key is 44 characters plus
/// a newline, so anything much bigger is not a key file.
const MAX_KEY_FILE_LEN: u64 = 1024;

/// The instance master key (key-encryption key for bank credentials).
///
/// It does not implement `Debug`, `Display`, `Clone` or `Serialize`, so it
/// cannot reach a log or a response by accident, and its bytes are wiped
/// from memory on drop.
pub struct MasterKey(Zeroizing<[u8; MASTER_KEY_LEN]>);

impl MasterKey {
    /// Reads a key file: 32 bytes in standard base64 (surrounding
    /// whitespace ignored), as written by `head -c 32 /dev/urandom | base64`.
    pub fn from_file(path: &Path) -> Result<Self, KeyError> {
        let file = std::fs::File::open(path).map_err(|err| KeyError::Unreadable(err.kind()))?;
        let metadata = file
            .metadata()
            .map_err(|err| KeyError::Unreadable(err.kind()))?;
        if !metadata.is_file() {
            return Err(KeyError::NotAFile);
        }
        let mut text = Zeroizing::new(String::new());
        file.take(MAX_KEY_FILE_LEN + 1)
            .read_to_string(&mut text)
            .map_err(|_| KeyError::Malformed)?;
        if text.len() as u64 > MAX_KEY_FILE_LEN {
            return Err(KeyError::Malformed);
        }
        Self::from_base64(&text)
    }

    /// Decodes a key from standard base64 (surrounding whitespace ignored).
    pub fn from_base64(text: &str) -> Result<Self, KeyError> {
        let bytes = Zeroizing::new(
            STANDARD
                .decode(text.trim())
                .map_err(|_| KeyError::Malformed)?,
        );
        let mut key = Zeroizing::new([0_u8; MASTER_KEY_LEN]);
        if bytes.len() != MASTER_KEY_LEN {
            return Err(KeyError::Malformed);
        }
        key.copy_from_slice(&bytes);
        Ok(MasterKey(key))
    }

    /// The key in its file form (standard base64, no newline), for the
    /// explicit `admin export-key` command only. Wiped on drop.
    pub fn export_base64(&self) -> Zeroizing<String> {
        Zeroizing::new(STANDARD.encode(self.0.as_slice()))
    }
}

/// Why a key could not be loaded. Never contains key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyError {
    /// The file cannot be opened or read.
    Unreadable(std::io::ErrorKind),
    /// The path is not a regular file.
    NotAFile,
    /// The content is not 32 bytes in standard base64.
    Malformed,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Unreadable(kind) => write!(f, "cannot read the key file ({kind})"),
            KeyError::NotAFile => f.write_str("the key path is not a regular file"),
            KeyError::Malformed => f.write_str(
                "the key file does not hold 32 bytes in standard base64 \
                 (generate one with `head -c 32 /dev/urandom | base64`)",
            ),
        }
    }
}

impl std::error::Error for KeyError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// An obviously fake key: bytes 0..32, built at run time so no
    /// key-shaped literal sits in the source.
    fn fake_key() -> String {
        STANDARD.encode((0..32).collect::<Vec<u8>>())
    }

    #[test]
    fn round_trips_base64() {
        let fake = fake_key();
        let key = MasterKey::from_base64(&format!("  {fake}\n")).expect("valid key");
        assert_eq!(key.export_base64().as_str(), fake);
        let bytes: Vec<u8> = (0..32).collect();
        assert_eq!(key.0.as_slice(), bytes.as_slice());
    }

    #[test]
    fn rejects_malformed_keys() {
        let short = STANDARD.encode([7_u8; 31]);
        for bad in ["", "not base64!", short.as_str(), "AAAA"] {
            assert!(
                matches!(MasterKey::from_base64(bad), Err(KeyError::Malformed)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn reads_files_and_explains_failures() {
        let dir = std::env::temp_dir().join(format!("solde-key-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("master.key");
        let fake = fake_key();
        std::fs::write(&path, format!("{fake}\n")).expect("write key");
        let key = MasterKey::from_file(&path).expect("load");
        assert_eq!(key.export_base64().as_str(), fake);

        assert!(matches!(
            MasterKey::from_file(&dir.join("missing.key")),
            Err(KeyError::Unreadable(std::io::ErrorKind::NotFound))
        ));
        assert!(matches!(
            MasterKey::from_file(&dir),
            Err(KeyError::NotAFile) | Err(KeyError::Unreadable(_))
        ));
        std::fs::write(&path, "x".repeat(2048)).expect("write junk");
        assert!(matches!(
            MasterKey::from_file(&path),
            Err(KeyError::Malformed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        // The error text never echoes the content.
        assert!(!KeyError::Malformed.to_string().contains(&fake));
    }
}
