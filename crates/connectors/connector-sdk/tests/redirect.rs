// SPDX-License-Identifier: AGPL-3.0-or-later
//! The redirect (consent page) round trip, as an Enable Banking style
//! connector would run it: build the consent URL from the context's redirect
//! target, ask the user, verify the callback, read the code.

// The SDK's own tests exercise what bank crates may not use (building
// clients, local servers, temp files); the bans target bank crates only.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use solde_connector_sdk::http::Url;
use solde_connector_sdk::testkit::{Scripted, ScriptedInteraction, block_on};
use solde_connector_sdk::{
    Challenge, ChallengeResponse, ConnectorError, CredentialSet, LoginContext, RedirectTarget,
    SecretString,
};

const STATE: &str = "state-0123456789abcdef";

fn target() -> RedirectTarget {
    RedirectTarget::new(
        Url::parse("https://solde.invalid/api/v1/connectors/demo/callback").expect("valid URL"),
        SecretString::new(STATE),
    )
}

/// What a connector does: returns the authorization code.
async fn consent(ctx: &LoginContext<'_>) -> Result<String, ConnectorError> {
    let target = ctx.redirect_target().ok_or(ConnectorError::ScaRequired)?;
    let mut url = Url::parse("https://aggregator.invalid/auth").expect("valid URL");
    url.query_pairs_mut()
        .append_pair("redirect_url", target.callback_url.as_str())
        .append_pair("state", target.state.expose());
    let answer = ctx
        .challenge(Challenge::Redirect {
            label: "Approve access in your bank".to_owned(),
            url: url.to_string(),
        })
        .await?;
    let ChallengeResponse::Redirected { callback_url } = answer else {
        return Err(ConnectorError::broken("unexpected answer to a redirect"));
    };
    let params = ctx.verify_callback(&callback_url)?;
    Ok(params.expose_required("code")?.to_owned())
}

fn run(callback: &str) -> (Result<String, ConnectorError>, Vec<Challenge>) {
    let io = ScriptedInteraction::new(vec![Scripted::Redirected(callback.to_owned())]);
    let ctx = LoginContext::interactive(&io, CredentialSet::new()).with_redirect_target(target());
    let result = block_on(consent(&ctx));
    (result, io.challenges())
}

#[test]
fn round_trip() {
    let callback = format!(
        "https://solde.invalid/api/v1/connectors/demo/callback?code=auth-code-1&state={STATE}"
    );
    let (result, challenges) = run(&callback);
    assert_eq!(result, Ok("auth-code-1".to_owned()));
    let Challenge::Redirect { url, .. } = &challenges[0] else {
        panic!("{challenges:?}");
    };
    assert!(url.contains(&format!("state={STATE}")), "{url}");
    assert!(
        url.contains("redirect_url=https%3A%2F%2Fsolde.invalid"),
        "{url}"
    );
}

#[test]
fn rejected_callbacks() {
    let base = "https://solde.invalid/api/v1/connectors/demo/callback";
    for (callback, reason) in [
        (
            format!("{base}?code=c&state=wrong-state-0000000000"),
            "state mismatch",
        ),
        (format!("{base}?code=c"), "missing state"),
        (
            format!("{base}?code=c&state={STATE}&state={STATE}"),
            "duplicate query parameter",
        ),
        (
            format!("https://evil.invalid/api/v1/connectors/demo/callback?code=c&state={STATE}"),
            "not the expected callback URL",
        ),
        (
            format!("{base}/other?code=c&state={STATE}"),
            "not the expected callback URL",
        ),
        (format!("{base}?state={STATE}"), "missing `code` parameter"),
    ] {
        let (result, _) = run(&callback);
        assert_eq!(
            result,
            Err(ConnectorError::CallbackRejected {
                reason: reason.to_owned()
            }),
            "{callback}"
        );
    }
}

#[test]
fn unattended_cannot_redirect() {
    let ctx = LoginContext::unattended().with_redirect_target(target());
    assert_eq!(block_on(consent(&ctx)), Err(ConnectorError::ScaRequired));
}
