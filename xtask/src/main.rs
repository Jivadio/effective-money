// SPDX-License-Identifier: AGPL-3.0-or-later
//! Repository automation, run with `cargo xtask <command>`.

mod check_deps;
mod rulesets;

use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  check-deps              enforce the crate dependency rules
  rulesets check          compare .github/rulesets/main.json to the live ruleset
  rulesets apply          create/update the live ruleset from .github/rulesets/main.json
  openapi                 not implemented yet (planned with the REST API)
  schema-parity           not implemented yet (planned with the storage layer)
  fixtures-scrub-check    not implemented yet (planned with the direct connectors)
  connector-smoke <bank>  not implemented yet (planned with the direct connectors)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["check-deps"] => check_deps::run(),
        ["rulesets", "check"] => rulesets::check(),
        ["rulesets", "apply"] => rulesets::apply(),
        ["openapi", ..] => not_implemented("openapi", "the REST API lands (OpenAPI drift check)"),
        ["schema-parity", ..] => not_implemented(
            "schema-parity",
            "the storage layer lands (Postgres/SQLite schema parity)",
        ),
        ["fixtures-scrub-check", ..] => {
            not_implemented("fixtures-scrub-check", "the direct bank connectors land")
        }
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
