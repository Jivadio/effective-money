// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo xtask rulesets {check,apply}`: the `main` branch ruleset as code.
//!
//! The source of truth is `.github/rulesets/main.json`. `check` compares it to
//! the live ruleset (GitHub API via `gh`); `apply` creates or updates it.
//! The repository is `$GITHUB_REPOSITORY`, or the one `gh repo view` reports.

use std::path::PathBuf;
use std::process::Command;

use anyhow::Context as _;
use serde_json::Value;

/// Top-level ruleset fields that are compared and applied.
const FIELDS: &[&str] = &[
    "name",
    "target",
    "enforcement",
    "conditions",
    "bypass_actors",
    "rules",
];

fn ruleset_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.github/rulesets/main.json")
}

fn load_expected() -> anyhow::Result<Value> {
    let path = ruleset_path();
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn gh(args: &[&str]) -> anyhow::Result<Value> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .context("cannot run `gh` (GitHub CLI)")?;
    if !output.status.success() {
        anyhow::bail!(
            "`gh {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&output.stdout).context("gh returned invalid JSON")
}

fn repository() -> anyhow::Result<String> {
    if let Ok(repo) = std::env::var("GITHUB_REPOSITORY")
        && !repo.is_empty()
    {
        return Ok(repo);
    }
    let v = gh(&["repo", "view", "--json", "nameWithOwner"])?;
    v["nameWithOwner"]
        .as_str()
        .map(str::to_owned)
        .context("cannot determine the repository")
}

/// Returns the id of the live ruleset named `name`, if any.
fn find_live_id(repo: &str, name: &str) -> anyhow::Result<Option<u64>> {
    let list = gh(&[
        "api",
        &format!("repos/{repo}/rulesets?includes_parents=false"),
    ])?;
    Ok(list
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["name"] == name)
        .and_then(|r| r["id"].as_u64()))
}

/// `true` if `live` contains everything in `expected`.
///
/// Objects: every expected key must match (extra live keys are API defaults).
/// Arrays: same length and an order-insensitive one-to-one match.
pub fn matches(expected: &Value, live: &Value) -> bool {
    match (expected, live) {
        (Value::Object(e), Value::Object(l)) => e
            .iter()
            .all(|(k, ev)| l.get(k).is_some_and(|lv| matches(ev, lv))),
        (Value::Array(e), Value::Array(l)) => {
            if e.len() != l.len() {
                return false;
            }
            let mut used = vec![false; l.len()];
            e.iter().all(|ev| {
                let found = l
                    .iter()
                    .enumerate()
                    .find(|(i, lv)| !used[*i] && matches(ev, lv));
                match found {
                    Some((i, _)) => {
                        used[i] = true;
                        true
                    }
                    None => false,
                }
            })
        }
        _ => expected == live,
    }
}

fn subset(value: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for field in FIELDS {
        if let Some(v) = value.get(*field) {
            out.insert((*field).to_owned(), v.clone());
        }
    }
    Value::Object(out)
}

pub fn check() -> anyhow::Result<()> {
    let expected = subset(&load_expected()?);
    let name = expected["name"]
        .as_str()
        .context("ruleset file has no `name`")?
        .to_owned();
    let repo = repository()?;
    let id = find_live_id(&repo, &name)?
        .with_context(|| format!("no live ruleset named `{name}` on {repo}"))?;
    let live = subset(&gh(&["api", &format!("repos/{repo}/rulesets/{id}")])?);
    if matches(&expected, &live) {
        println!("rulesets check: `{name}` on {repo} matches .github/rulesets/main.json");
        return Ok(());
    }
    eprintln!(
        "expected:\n{}\nlive:\n{}",
        serde_json::to_string_pretty(&expected)?,
        serde_json::to_string_pretty(&live)?
    );
    anyhow::bail!("live ruleset `{name}` on {repo} differs from .github/rulesets/main.json")
}

pub fn apply() -> anyhow::Result<()> {
    let expected = load_expected()?;
    let name = expected["name"]
        .as_str()
        .context("ruleset file has no `name`")?
        .to_owned();
    let repo = repository()?;
    let path = ruleset_path();
    let input = path.to_str().context("non UTF-8 path")?;
    match find_live_id(&repo, &name)? {
        Some(id) => {
            gh(&[
                "api",
                "-X",
                "PUT",
                &format!("repos/{repo}/rulesets/{id}"),
                "--input",
                input,
            ])?;
            println!("rulesets apply: updated `{name}` (id {id}) on {repo}");
        }
        None => {
            gh(&[
                "api",
                "-X",
                "POST",
                &format!("repos/{repo}/rulesets"),
                "--input",
                input,
            ])?;
            println!("rulesets apply: created `{name}` on {repo}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_parses_and_has_fields() {
        let v = load_expected().unwrap();
        assert_eq!(v["name"], "main");
        assert!(v["rules"].is_array());
    }

    #[test]
    fn subset_match_ignores_extra_live_keys_and_order() {
        let expected = json!({"rules": [{"type": "deletion"}, {"type": "pull_request", "parameters": {"required_approving_review_count": 0}}]});
        let live = json!({"id": 1, "rules": [
            {"type": "pull_request", "parameters": {"required_approving_review_count": 0, "automatic_copilot_code_review_enabled": false}},
            {"type": "deletion"}
        ]});
        assert!(matches(&expected, &live));
    }

    #[test]
    fn mismatch_detected() {
        let expected = json!({"rules": [{"type": "pull_request", "parameters": {"required_approving_review_count": 0}}]});
        let changed = json!({"rules": [{"type": "pull_request", "parameters": {"required_approving_review_count": 1}}]});
        let extra_rule = json!({"rules": [{"type": "pull_request", "parameters": {"required_approving_review_count": 0}}, {"type": "deletion"}]});
        assert!(!matches(&expected, &changed));
        assert!(!matches(&expected, &extra_rule));
        assert!(!matches(&expected, &json!({})));
    }
}
