// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runs the real binary: startup refusals and `admin export-key`.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_solde-server");

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solde-server-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The binary with a clean `SOLDE_*` environment.
fn server(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command.args(args);
    for (name, _) in std::env::vars() {
        if name.starts_with("SOLDE_") {
            command.env_remove(name);
        }
    }
    command.envs(env.iter().copied());
    command.output().expect("run solde-server")
}

/// 32 fake bytes in the keygen file format (`base64` output + newline).
fn write_fake_key(dir: &std::path::Path) -> (PathBuf, String) {
    // Base64 of bytes 1..=32, assembled at run time.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = (1..=32).collect();
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (i, b)| acc | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(char::from(alphabet[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text.push('\n');
    let path = dir.join("master.key");
    std::fs::write(&path, &text).expect("write key");
    (path, text)
}

#[test]
fn refuses_to_start_without_a_key() {
    let dir = temp_dir("nokey");
    let db = format!("sqlite://{}", dir.join("solde.db").display());
    let output = server(&[], &[("SOLDE_DATABASE_URL", &db)]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no master key"), "{stderr}");
    assert!(stderr.contains("SOLDE_MASTER_KEY_FILE"), "{stderr}");
    assert!(
        !dir.join("solde.db").exists(),
        "refused before opening the database"
    );

    let missing = dir.join("missing.key");
    let output = server(
        &[],
        &[
            ("SOLDE_DATABASE_URL", &db),
            ("SOLDE_MASTER_KEY_FILE", &missing.display().to_string()),
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot load the master key"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn refuses_to_start_without_a_database_url() {
    let dir = temp_dir("nodb");
    let (key, _) = write_fake_key(&dir);
    let output = server(
        &[],
        &[("SOLDE_MASTER_KEY_FILE", &key.display().to_string())],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("SOLDE_DATABASE_URL"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_key_prints_the_key_file() {
    let dir = temp_dir("export");
    let (key, text) = write_fake_key(&dir);
    let output = server(
        &["admin", "export-key"],
        &[("SOLDE_MASTER_KEY_FILE", &key.display().to_string())],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), text);
    assert!(output.stderr.is_empty(), "nothing else is printed");

    let output = server(&["admin", "export-key"], &[]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_command_shows_usage() {
    let output = server(&["nope"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));
}

#[test]
fn starts_with_a_key_and_never_logs_it() {
    use std::io::{BufRead as _, BufReader};
    use std::process::Stdio;

    let dir = temp_dir("start");
    let (key, text) = write_fake_key(&dir);
    let db = dir.join("solde.db");
    let mut command = Command::new(BIN);
    for (name, _) in std::env::vars() {
        if name.starts_with("SOLDE_") || name == "RUST_LOG" {
            command.env_remove(name);
        }
    }
    let mut child = command
        .env("SOLDE_MASTER_KEY_FILE", &key)
        .env("SOLDE_DATABASE_URL", format!("sqlite://{}", db.display()))
        .env("SOLDE_LISTEN_ADDR", "127.0.0.1:0")
        .env("SOLDE_INSECURE_COOKIES", "1")
        .env("RUST_LOG", "debug")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn solde-server");
    let stdout = child.stdout.take().expect("stdout");
    let mut log = String::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("log line");
        log.push_str(&line);
        log.push('\n');
        if line.contains("listening") {
            break;
        }
    }
    let _ = child.kill();
    let output = child.wait_with_output().expect("wait");
    log.push_str(&String::from_utf8_lossy(&output.stderr));

    assert!(log.contains("listening"), "{log}");
    assert!(log.contains("database ready"), "migrations ran: {log}");
    assert!(log.contains("SOLDE_INSECURE_COOKIES"), "warns: {log}");
    assert!(!log.contains(text.trim()), "the key never reaches the logs");
    assert!(db.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
