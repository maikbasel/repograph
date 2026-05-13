//! Shared helpers for repograph acceptance tests.
//!
//! `mod.rs` (rather than `common.rs`) prevents Cargo from compiling this as a
//! standalone test binary; other tests pull it in via `mod common;`.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use git2::{Repository, Signature};

/// Build a `repograph` command with `REPOGRAPH_CONFIG_DIR` pointed at `config_dir`.
/// Strips any inherited `REPOGRAPH_CONFIG_DIR` first so the test always controls it.
pub fn repograph_cmd(config_dir: &Path) -> Command {
    assert!(
        config_dir.is_absolute(),
        "test bug: config_dir must be absolute, got {}",
        config_dir.display()
    );
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR")
        .env("REPOGRAPH_CONFIG_DIR", config_dir);
    cmd
}

/// Build a `repograph` command that uses the `--config-dir <path>` flag instead
/// of the env var. Strips any inherited env var so the flag is the only signal.
pub fn repograph_cmd_with_flag(config_dir: &Path) -> Command {
    assert!(
        config_dir.is_absolute(),
        "test bug: config_dir must be absolute, got {}",
        config_dir.display()
    );
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR")
        .arg("--config-dir")
        .arg(config_dir);
    cmd
}

/// Build a command with no config-dir hint at all (no flag, env var stripped).
/// Used by the platform-no-default scenario.
pub fn repograph_cmd_no_config_hint() -> Command {
    let mut cmd = Command::cargo_bin("repograph").expect("repograph binary built");
    cmd.env_remove("REPOGRAPH_CONFIG_DIR");
    cmd
}

/// Initialize a real git repository at `parent.join(name)` with one commit so
/// HEAD exists. Returns the canonicalized absolute path.
pub fn fixture_git_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).expect("create fixture dir");
    let repo = Repository::init(&path).expect("git init");
    let sig = Signature::now("Test", "test@example.com").expect("signature");
    let tree_id = {
        let mut index = repo.index().expect("repo index");
        index.write_tree().expect("write empty tree")
    };
    {
        let tree = repo.find_tree(tree_id).expect("find empty tree");
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .expect("initial commit");
    }
    drop(repo);
    std::fs::canonicalize(&path).expect("canonicalize fixture path")
}

/// Parse JSON output from `repograph list --json` and return the `repos` array.
pub fn parse_repos_json(stdout: &[u8]) -> Vec<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_slice(stdout).expect("stdout is valid JSON");
    parsed
        .get("repos")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("envelope contains a `repos` array")
}

/// Parse the full JSON envelope from `repograph list --json`.
pub fn parse_list_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}

/// Parse JSON output from `repograph workspace ls --json` and return the
/// `workspaces` array.
pub fn parse_workspaces_json(stdout: &[u8]) -> Vec<serde_json::Value> {
    let parsed: serde_json::Value = serde_json::from_slice(stdout).expect("stdout is valid JSON");
    parsed
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("envelope contains a `workspaces` array")
}

/// Parse the full JSON envelope from `repograph workspace show --json`.
pub fn parse_workspace_show_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}
