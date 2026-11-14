// SPDX-License-Identifier: AGPL-3.0-or-later
//! Repository automation, run with `cargo xtask <command>`.

mod check_deps;
mod connector_lint;
mod fixtures_scrub;
mod openapi;
mod rulesets;
mod schema_parity;

use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  check-deps              enforce the crate dependency rules and connector guardrails
  fixtures-scrub-check    look for secrets or personal data in connector fixtures
  clippy-connectors       clippy on bank connector crates with disallowed_* lints forbidden
  rulesets check          compare .github/rulesets/main.json to the live ruleset
  rulesets apply          create/update the live ruleset from .github/rulesets/main.json
  openapi [--check]       write (or check) apps/web/src/api/openapi.json from the API
  schema-parity           check that the Postgres and SQLite migrations define the same schema
  connector-smoke <bank>  not implemented yet (planned with the direct connectors)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["check-deps"] => check_deps::run(),
        ["rulesets", "check"] => rulesets::check(),
        ["rulesets", "apply"] => rulesets::apply(),
        ["openapi"] => openapi::run(check_deps::workspace_root(), false),
        ["openapi", "--check"] => openapi::run(check_deps::workspace_root(), true),
        ["schema-parity"] => schema_parity::run(check_deps::workspace_root()),
        ["fixtures-scrub-check"] => fixtures_scrub::run(check_deps::workspace_root()),
        ["clippy-connectors"] => connector_lint::run_clippy(check_deps::workspace_root()),
        ["connector-smoke", ..] => {
            not_implemented("connector-smoke", "the direct bank connectors land")
        }
        ["help" | "--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn not_implemented(command: &str, milestone: &str) -> anyhow::Result<()> {
    anyhow::bail!("`{command}` is not implemented until {milestone}")
}
