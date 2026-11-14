// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo xtask openapi [--check]`: the OpenAPI document of the REST API.
//!
//! The document is generated from the handlers (utoipa) by the
//! `solde-api` example `openapi`, and committed as [`SPEC_PATH`] so the web
//! app can generate its typed client from it. Without `--check` the file is
//! rewritten; with `--check` (CI) it must already match, so an API change
//! cannot land without its updated description.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, bail};

/// Where the generated document is committed, relative to the workspace.
pub const SPEC_PATH: &str = "apps/web/src/api/openapi.json";

pub fn run(root: &Path, check: bool) -> anyhow::Result<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "run",
            "--quiet",
            "--locked",
            "--package",
            "solde-api",
            "--example",
            "openapi",
        ])
        .current_dir(root)
        .output()
        .context("cannot run cargo")?;
    if !output.status.success() {
        bail!(
            "generating the OpenAPI document failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let generated = String::from_utf8(output.stdout).context("OpenAPI output is not UTF-8")?;
    let path = root.join(SPEC_PATH);
    if check {
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        if committed != generated {
            bail!(
                "{SPEC_PATH} is out of date with the API: run `cargo xtask openapi` and commit it"
            );
        }
        println!("openapi: {SPEC_PATH} is up to date");
    } else {
        std::fs::write(&path, generated).with_context(|| format!("cannot write {SPEC_PATH}"))?;
        println!("openapi: wrote {SPEC_PATH}");
    }
    Ok(())
}
