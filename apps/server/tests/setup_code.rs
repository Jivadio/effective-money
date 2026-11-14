// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one-time setup code: generated only while the instance is not set up.

use solde_services::NewMemberInput;

#[tokio::test]
async fn setup_code_only_before_setup() {
    let store = solde_services::connect("sqlite::memory:")
        .await
        .expect("in-memory store");
    let code = solde_server::setup_code(&*store)
        .await
        .expect("setup code")
        .expect("not set up yet");
    assert_eq!(code.expose().len(), 19, "XXXX-XXXX-XXXX-XXXX");
    let again = solde_server::setup_code(&*store)
        .await
        .expect("setup code")
        .expect("still not set up");
    assert_ne!(code.expose(), again.expose(), "a new code per start");

    solde_services::bootstrap(
        &*store,
        "Test home",
        NewMemberInput {
            name: "Alice Test".to_owned(),
            email: "alice@example.test".to_owned(),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$ZmFrZXNhbHQ$ZmFrZWhhc2g".to_owned(),
        },
    )
    .await
    .expect("bootstrap");
    assert!(
        solde_server::setup_code(&*store)
            .await
            .expect("setup code")
            .is_none(),
        "no code once set up"
    );
}
