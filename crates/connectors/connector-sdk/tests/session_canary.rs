// SPDX-License-Identifier: AGPL-3.0-or-later
//! `assert_session_has_no_credentials` catches the canary in every encoding
//! a careless connector might use to stash credentials in its session.

use solde_connector_sdk::testkit::{CANARY, assert_session_has_no_credentials};
use solde_connector_sdk::{
    Capabilities, ConnectorManifest, CredentialField, SessionBlob, Stability,
};

fn manifest() -> ConnectorManifest {
    ConnectorManifest {
        id: "demo",
        display_name: "Demo",
        capabilities: Capabilities::none().with_accounts(),
        credential_fields: vec![CredentialField {
            name: "password",
            label: "Password",
            secret: true,
        }],
        stability: Stability::Experimental,
    }
}

fn std_b64(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(A[((n >> (18 - 6 * i)) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn caught(session: &str) -> bool {
    let blob = SessionBlob::new(session.as_bytes().to_vec());
    std::panic::catch_unwind(|| assert_session_has_no_credentials(&blob, &manifest())).is_err()
}

#[test]
fn clean_session_passes() {
    assert!(!caught(r#"{"token":"tok-123","valid_until":"2026-04-15"}"#));
    // A token that merely carries the canary marker is not a credential.
    assert!(!caught(&format!(r#"{{"token":"tok-{CANARY}"}}"#)));
}

#[test]
fn stashed_credentials_are_caught() {
    let password = format!("password-{CANARY}");
    let hex: String = password.bytes().map(|b| format!("{b:02X}")).collect();
    for prefix in ["", "a", "ab"] {
        let encoded = std_b64(format!("{prefix}{password}").as_bytes());
        assert!(
            caught(&format!(r#"{{"p":"{encoded}"}}"#)),
            "base64 offset {prefix:?}"
        );
        let url_safe = encoded.replace('+', "-").replace('/', "_");
        assert!(
            caught(&format!(r#"{{"p":"{url_safe}"}}"#)),
            "url-safe {prefix:?}"
        );
    }
    assert!(caught(&format!(r#"{{"p":"{password}"}}"#)), "raw");
    assert!(caught(&format!(r#"{{"p":"{hex}"}}"#)), "hex upper");
    assert!(
        caught(&format!(r#"{{"p":"{}"}}"#, hex.to_lowercase())),
        "hex lower"
    );
    assert!(
        caught(&format!(r#"{{"p":"{}"}}"#, password.replace('-', "%2D"))),
        "percent"
    );
}
