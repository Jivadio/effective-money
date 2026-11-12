// SPDX-License-Identifier: AGPL-3.0-or-later
//! Guardrails for bank connector crates (`crates/connectors/*` except the SDK
//! and the registry), run by `cargo xtask check-deps`.
//!
//! A connector must not be able to opt out of the other checks or run code at
//! build time. In its manifest it must not have a build script (`build.rs` or
//! `[package] build`), `links`, `proc-macro`, its own `clippy.toml`, a missing
//! `[lints] workspace = true`, or a non-dev dependency enabling the SDK's
//! `testkit` feature. Its Rust files are first normalized (comments and
//! string/char literal contents removed, whitespace dropped, lines joined),
//! then must not contain: the [`FORBIDDEN`] paths (`std::fs` is allowed in
//! `tests/` only), an `allow`/`expect`/`warn`/`cfg_attr` attribute naming
//! `warnings`, `clippy::all`, `clippy::style`, `clippy::disallowed_*` or
//! `unsafe_code`, or an alias of `std` or of its process/env/os/fs modules.
//!
//! `cargo xtask clippy-connectors` also runs clippy on bank crates with the
//! `disallowed_*` lints FORBIDDEN, which no in-crate attribute can lower.
//!
//! These are guardrails against mistakes and casual misuse, not a sandbox:
//! code review remains the real boundary.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use cargo_metadata::MetadataCommand;

/// Crates under `crates/connectors/` that are not bank connectors.
const EXEMPT: &[&str] = &["solde-connector-sdk", "solde-connector-registry"];

/// Paths that must not appear in a connector's (normalized) Rust code.
pub const FORBIDDEN: &[&str] = &[
    "std::process",
    "std::os::",
    "std::env",
    "externcrate",
    "#[path",
    "#![path",
    "include!(",
];

/// Like [`FORBIDDEN`], outside `tests/` only.
const FORBIDDEN_OUTSIDE_TESTS: &[&str] = &["std::fs"];

/// Lint names an attribute must not lower.
const PROTECTED_LINTS: &[&str] = &[
    "warnings",
    "clippy::all",
    "clippy::style",
    "clippy::disallowed",
    "unsafe_code",
];

/// The source with comments removed, string and char literals emptied
/// (`""`, `' '`), and whitespace collapsed to single spaces (`spaced`) or
/// removed (`compact`).
struct Normalized {
    compact: String,
    spaced: String,
}

fn normalize(source: &str) -> Normalized {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        if c == '/' && next == Some('*') {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        // Raw strings: r"..", r#".."#, and the byte/C forms br#".."#, cr#".."#
        // (whose `b`/`c` prefix was already copied to `out`).
        let is_word = |p: char| p.is_alphanumeric() || p == '_';
        let before_prefix = out
            .strip_suffix(['b', 'c'])
            .filter(|rest| !rest.ends_with(is_word));
        if c == 'r'
            && matches!(next, Some('"' | '#'))
            && (!out.ends_with(is_word) || before_prefix.is_some())
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                j += 1;
                loop {
                    match chars.get(j) {
                        None => break,
                        Some('"') if (0..hashes).all(|k| chars.get(j + 1 + k) == Some(&'#')) => {
                            j += 1 + hashes;
                            break;
                        }
                        Some(_) => j += 1,
                    }
                }
                out.push_str("\"\"");
                i = j;
                continue;
            }
        }
        if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push_str("\"\"");
            continue;
        }
        // Char literals ('a', '\n', '\u{..}'), not lifetimes ('a).
        if c == '\'' {
            let close = if next == Some('\\') {
                (i + 2..chars.len().min(i + 12)).find(|&k| chars[k] == '\'')
            } else if chars.get(i + 2) == Some(&'\'') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(end) = close {
                out.push_str("' '");
                i = end + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    let spaced = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();
    Normalized { compact, spaced }
}

/// Every `#[...]` / `#![...]` attribute body of the compact source.
fn attributes(compact: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = compact.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            if bytes.get(j) == Some(&b'!') {
                j += 1;
            }
            if bytes.get(j) == Some(&b'[') {
                let start = j + 1;
                let mut depth = 1;
                let mut k = start;
                while k < bytes.len() && depth > 0 {
                    match bytes[k] {
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        _ => {}
                    }
                    k += 1;
                }
                out.push(&compact[start..k.saturating_sub(1).max(start)]);
                i = k;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Problems in one Rust file's source.
fn check_source(source: &str, in_tests: bool) -> Vec<String> {
    let n = normalize(source);
    let mut v = Vec::new();
    for pattern in FORBIDDEN {
        if n.compact.contains(pattern) {
            v.push(format!("`{pattern}` is not allowed"));
        }
    }
    if !in_tests {
        for pattern in FORBIDDEN_OUTSIDE_TESTS {
            if n.compact.contains(pattern) {
                v.push(format!("`{pattern}` is not allowed outside tests/"));
            }
        }
    }
    for attr in attributes(&n.compact) {
        let lowers = ["allow(", "expect(", "warn(", "cfg_attr("]
            .iter()
            .any(|a| attr.starts_with(a) || (attr.starts_with("cfg_attr(") && attr.contains(a)));
        if lowers {
            for lint in PROTECTED_LINTS {
                if attr.contains(lint) {
                    v.push(format!("attribute `#[{attr}]` lowers `{lint}`"));
                }
            }
        }
    }
    // Aliases of std, or of its process/env/os/fs modules.
    let spaced = &n.spaced;
    for (i, _) in spaced.match_indices("use ") {
        let stmt = &spaced[i..spaced[i..].find(';').map_or(spaced.len(), |e| i + e)];
        let compact: String = stmt.chars().filter(|c| !c.is_whitespace()).collect();
        let path = compact.trim_start_matches("use").trim_start_matches("::");
        // `use {std as s, std::fmt};` imports several roots at once.
        for member in root_members(path) {
            let member = member.trim_start_matches("::");
            if !member.starts_with("std") {
                continue;
            }
            let alias_of_std = member.starts_with("stdas") || member.contains("selfas");
            let module = ["process", "env", "os", "fs"].iter().any(|m| {
                member.starts_with("std::{")
                    && member
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .any(|w| w == *m)
            });
            if alias_of_std || module {
                v.push(format!("`{stmt}` aliases std or a forbidden module"));
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// The top-level paths of a compact `use` tree: `{a,b::{c,d}}` gives `a` and
/// `b::{c,d}`; a path without a leading group is returned as is.
fn root_members(path: &str) -> Vec<&str> {
    let Some(inner) = path.strip_prefix('{').and_then(|p| p.strip_suffix('}')) else {
        return vec![path];
    };
    let mut members = Vec::new();
    let (mut depth, mut start) = (0_usize, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                members.extend(root_members(&inner[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    members.extend(root_members(&inner[start..]));
    members
}

/// Every violation of the bank connector crate at `dir` (empty = OK).
pub fn check_crate(dir: &Path) -> Vec<String> {
    let name = dir.display().to_string();
    let mut v = Vec::new();
    if dir.join("build.rs").exists() {
        v.push(format!(
            "{name}: build.rs is not allowed in a connector crate"
        ));
    }
    for file in ["clippy.toml", ".clippy.toml"] {
        if dir.join(file).exists() {
            v.push(format!("{name}: its own {file} is not allowed (it would replace crates/connectors/clippy.toml)"));
        }
    }
    match std::fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
    {
        Some(manifest) => check_manifest(&name, &manifest, &mut v),
        None => v.push(format!("{name}: cannot read Cargo.toml")),
    }
    for file in rust_files(dir) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            v.push(format!("{}: cannot read", file.display()));
            continue;
        };
        let in_tests = file
            .strip_prefix(dir)
            .is_ok_and(|rel| rel.starts_with("tests"));
        for problem in check_source(&text, in_tests) {
            v.push(format!(
                "{}: {problem} in a connector crate",
                file.display()
            ));
        }
    }
    v
}

fn check_manifest(name: &str, manifest: &toml::Table, v: &mut Vec<String>) {
    let package = manifest.get("package").and_then(toml::Value::as_table);
    for key in ["build", "links"] {
        if package.is_some_and(|p| p.contains_key(key)) {
            v.push(format!(
                "{name}: [package] {key} is not allowed in a connector crate"
            ));
        }
    }
    let proc_macro = manifest
        .get("lib")
        .and_then(|l| l.get("proc-macro").or_else(|| l.get("proc_macro")))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    if proc_macro {
        v.push(format!("{name}: a connector crate cannot be a proc-macro"));
    }
    let workspace_lints = manifest
        .get("lints")
        .and_then(|l| l.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    if !workspace_lints {
        v.push(format!("{name}: [lints] workspace = true is required"));
    }
    // Normal and build dependencies, including target-specific ones.
    let mut tables: Vec<&toml::Table> = Vec::new();
    for key in ["dependencies", "build-dependencies", "build_dependencies"] {
        tables.extend(manifest.get(key).and_then(toml::Value::as_table));
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            for key in ["dependencies", "build-dependencies", "build_dependencies"] {
                tables.extend(target.get(key).and_then(toml::Value::as_table));
            }
        }
    }
    for table in tables {
        for (dep, spec) in table {
            let package = spec.get("package").and_then(toml::Value::as_str);
            if dep != "solde-connector-sdk" && package != Some("solde-connector-sdk") {
                continue;
            }
            let testkit = spec
                .get("features")
                .and_then(toml::Value::as_array)
                .is_some_and(|f| f.iter().any(|x| x.as_str() == Some("testkit")));
            if testkit {
                v.push(format!(
                    "{name}: the SDK `testkit` feature is for [dev-dependencies] only"
                ));
            }
        }
    }
}

/// Every `*.rs` file of the crate, outside `target/` and `fixtures/`.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            if path.is_dir() {
                if file_name != "target" && file_name != "fixtures" {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// `(package name, directory)` of every bank connector crate of the workspace.
pub fn bank_crates(root: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .context("cargo metadata failed")?;
    let connectors_dir = root.join("crates/connectors");
    let mut out = Vec::new();
    for package in metadata.workspace_packages() {
        let dir = package
            .manifest_path
            .parent()
            .map(|p| p.as_std_path().to_path_buf())
            .unwrap_or_default();
        if dir.starts_with(&connectors_dir) && !EXEMPT.contains(&package.name.as_str()) {
            out.push((package.name.to_string(), dir));
        }
    }
    Ok(out)
}

/// Checks every bank connector crate of the workspace at `root`.
pub fn check_workspace(root: &Path) -> anyhow::Result<Vec<String>> {
    Ok(bank_crates(root)?
        .iter()
        .flat_map(|(_, dir)| check_crate(dir))
        .collect())
}

/// `cargo xtask clippy-connectors`: clippy on the bank crates with the
/// `disallowed_*` lints forbidden, so no `#[allow]` inside a crate can lower
/// them.
pub fn run_clippy(root: &Path) -> anyhow::Result<()> {
    let crates = bank_crates(root)?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cmd = std::process::Command::new(cargo);
    cmd.current_dir(root)
        .args(["clippy", "--locked", "--all-targets", "--no-deps"]);
    for (name, _) in &crates {
        cmd.args(["-p", name]);
    }
    cmd.args([
        "--",
        "-D",
        "warnings",
        "-F",
        "clippy::disallowed_types",
        "-F",
        "clippy::disallowed_methods",
    ]);
    let status = cmd.status().context("cannot run cargo clippy")?;
    anyhow::ensure!(
        status.success(),
        "clippy failed on the bank connector crates"
    );
    println!("clippy-connectors: OK ({} bank crates)", crates.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_MANIFEST: &str = "[package]\nname = \"solde-connector-x\"\nversion = \"0.0.0\"\n\n\
        [dependencies]\nsolde-connector-sdk = { path = \"../connector-sdk\" }\n\n\
        [dev-dependencies]\nsolde-connector-sdk = { path = \"../connector-sdk\", features = [\"testkit\"] }\n\n\
        [lints]\nworkspace = true\n";

    fn temp_crate(tag: &str, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("solde-connector-lint-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn check_and_clean(dir: PathBuf) -> Vec<String> {
        let v = check_crate(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        v
    }

    #[test]
    fn clean_crate_passes() {
        let dir = temp_crate(
            "good",
            GOOD_MANIFEST,
            &[
                (
                    "src/lib.rs",
                    "//! Fine.\nconst F: &str = include_str!(\"../fixtures/a.json\");\n",
                ),
                (
                    "fixtures/a.json",
                    "{\"note\": \"std::env in data is ignored\"}",
                ),
            ],
        );
        assert_eq!(check_and_clean(dir), Vec::<String>::new());
    }

    #[test]
    fn manifest_escapes_fail() {
        let manifest = "[package]\nname = \"solde-connector-x\"\nversion = \"0.0.0\"\n\
            build = \"gen.rs\"\nlinks = \"foo\"\n\n[lib]\nproc-macro = true\n\n\
            [dependencies]\nsolde-connector-sdk = { path = \"../connector-sdk\", features = [\"testkit\"] }\n\n\
            [target.'cfg(unix)'.build-dependencies]\nsdk = { package = \"solde-connector-sdk\", path = \"../connector-sdk\", features = [\"testkit\"] }\n";
        let dir = temp_crate(
            "manifest",
            manifest,
            &[
                ("build.rs", "fn main() {}\n"),
                ("clippy.toml", "\n"),
                (".clippy.toml", "\n"),
            ],
        );
        let v = check_and_clean(dir);
        for expected in [
            "build.rs is not allowed",
            "own clippy.toml",
            "own .clippy.toml",
            "[package] build",
            "[package] links",
            "proc-macro",
            "[lints] workspace = true is required",
        ] {
            assert!(v.iter().any(|m| m.contains(expected)), "{expected}: {v:?}");
        }
        assert_eq!(
            v.iter().filter(|m| m.contains("testkit")).count(),
            2,
            "{v:?}"
        );
    }

    #[test]
    fn forbidden_source_patterns_fail() {
        let dir = temp_crate(
            "source",
            GOOD_MANIFEST,
            &[
                ("src/lib.rs", "mod sneaky;\n"),
                (
                    "src/sneaky.rs",
                    "fn a() { std :: process :: exit(1) }\n\
                     fn b() { let _ = std::env::var(\"X\"); }\nuse std::os::unix::net::UnixStream;\n\
                     extern  crate alloc;\n#[path = \"x.rs\"] mod x;\ninclude!(\"x.rs\");\n\
                     fn c() { let _ = std::fs::read(\"x\"); }\n",
                ),
                (
                    "tests/t.rs",
                    "fn t() { let _ = std::env::args(); let _ = std::fs::read(\"f\"); }\n",
                ),
            ],
        );
        let v = check_and_clean(dir);
        for pattern in FORBIDDEN {
            if *pattern != "#![path" {
                assert!(v.iter().any(|m| m.contains(pattern)), "{pattern}: {v:?}");
            }
        }
        assert!(
            v.iter()
                .any(|m| m.contains("tests/t.rs") && m.contains("std::env")),
            "{v:?}"
        );
        // std::fs is fine in tests/, not in src/.
        assert!(
            v.iter()
                .any(|m| m.contains("sneaky.rs") && m.contains("std::fs")),
            "{v:?}"
        );
        assert!(
            !v.iter()
                .any(|m| m.contains("tests/t.rs") && m.contains("std::fs")),
            "{v:?}"
        );
    }

    #[test]
    fn lint_opt_outs_fail() {
        for source in [
            "#![allow(clippy::disallowed_types)]",
            "#[expect(clippy::disallowed_methods)] fn f() {}",
            "#![allow(\n    clippy ::\n    disallowed_types\n)]",
            "# [ allow ( clippy :: disallowed_methods ) ] fn f() {}",
            "#![allow(warnings)]",
            "#![allow(clippy::style)]",
            "#![allow(clippy::all)]",
            "#![warn(unsafe_code)]",
            "#![cfg_attr(not(test), allow(clippy::disallowed_types))]",
            "#![cfg_attr(all(), expect(warnings))]",
        ] {
            assert!(
                !check_source(source, false).is_empty(),
                "not caught: {source}"
            );
        }
    }

    #[test]
    fn std_aliases_fail() {
        for source in [
            "use std as s; fn f() { s::process::exit(0) }",
            "use ::std as s;",
            "use std::{self as s};",
            "use std::{io, process};",
            "use std::{env::var, io};",
            "use std::{fs, io};",
            "use std::{os::unix, io};",
            "use {std as s, std::fmt}; fn f() { s::process::exit(1) }",
            "use ::{std as s};",
            "use {core::fmt, std::{io, env}};",
        ] {
            assert!(
                !check_source(source, false).is_empty(),
                "not caught: {source}"
            );
        }
    }

    #[test]
    fn comments_and_strings_neither_hide_nor_trigger() {
        // A URL in a string must not swallow the rest of the line.
        assert!(!check_source("let u = \"https://x\"; std::env::var(\"A\");", false).is_empty());
        // Mentions in comments and strings are fine.
        for source in [
            "// std::env is not used here\nfn f() {}",
            "/* #![allow(warnings)] */ fn f() {}",
            "const S: &str = \"std::process\";",
            "const R: &str = r#\"std::env \"quoted\"\"#;",
            "const B: &[u8] = br#\"say \"std::env\" now\"#;",
            "const C: &[u8] = br\"std::process\";",
            "use {core::fmt, std::io};",
            "fn f<'a>(x: &'a str) -> char { '\\'' }",
            "use std::{collections::BTreeMap, io};",
            "#[allow(dead_code)] fn f() {}",
        ] {
            assert_eq!(
                check_source(source, false),
                Vec::<String>::new(),
                "{source}"
            );
        }
    }

    #[test]
    fn real_connectors_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(check_workspace(root).unwrap(), Vec::<String>::new());
    }
}
