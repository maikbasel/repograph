//! Acceptance tests for the stdout/stderr output contract and exit codes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{fixture_git_repo, repograph_cmd};

#[test]
fn add_emits_diagnostics_only_to_stderr() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "clean");

    let out = repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("clean")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(
        stdout.trim().is_empty(),
        "stdout is empty for add success, got: {stdout:?}"
    );
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("clean"),
        "stderr confirms registration, got: {stderr:?}"
    );
}

#[test]
fn list_json_pipes_cleanly_to_jq_style_consumers() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "piped");
    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&r)
        .arg("--name")
        .arg("piped")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let stdout = &out.get_output().stdout;
    // Simulate `jq '.repos | length'` — must parse, must access .repos.
    let value: serde_json::Value =
        serde_json::from_slice(stdout).expect("stdout is valid JSON for jq");
    let len = value["repos"].as_array().unwrap().len();
    assert_eq!(len, 1);
}

#[test]
fn missing_required_argument_exits_with_code_2() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir).arg("add").assert().code(2);
}

#[test]
fn malformed_toml_exits_with_code_1() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "this is = = not valid toml [[[",
    )
    .unwrap();

    repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .code(1)
        .stderr(predicate::str::is_empty().not());
}

#[cfg(unix)]
#[test]
fn permission_denied_on_config_write_exits_with_code_4() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    // Make config dir read-only so save fails.
    let mut perms = std::fs::metadata(&config_dir).unwrap().permissions();
    perms.set_mode(0o500);
    std::fs::set_permissions(&config_dir, perms).unwrap();

    let repo = fixture_git_repo(tmp.path(), "perm");
    let result = repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("perm")
        .assert()
        .code(4);
    let stderr = String::from_utf8_lossy(&result.get_output().stderr).to_string();
    assert!(!stderr.is_empty(), "stderr explains permission failure");

    // Restore perms so TempDir can clean up.
    let mut restore = std::fs::metadata(&config_dir).unwrap().permissions();
    restore.set_mode(0o700);
    std::fs::set_permissions(&config_dir, restore).unwrap();
}

// The "platform has no default config dir" scenario (spec: Config persistence
// → Platform has no default config dir and no override) is covered by a unit
// test in `main.rs::tests` rather than here. On Linux/macOS, `dirs::config_dir`
// falls back to `getpwuid` even when HOME and XDG_CONFIG_HOME are cleared, so
// the integration test cannot force the None branch. See design.md "Resolved
// deviations" for the rationale.
