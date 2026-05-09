//! Acceptance tests for `--config-dir` precedence and global-flag behavior.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use tempfile::TempDir;

use crate::common::{fixture_git_repo, parse_repos_json, repograph_cmd_with_flag};

#[test]
fn config_dir_flag_overrides_env_var() {
    let tmp = TempDir::new().unwrap();
    let env_dir = tmp.path().join("env-dir");
    let flag_dir = tmp.path().join("flag-dir");
    let repo = fixture_git_repo(tmp.path(), "myrepo");

    // Set env to one path, pass flag with a different path.
    let mut cmd = assert_cmd::Command::cargo_bin("repograph").unwrap();
    cmd.env("REPOGRAPH_CONFIG_DIR", &env_dir)
        .arg("--config-dir")
        .arg(&flag_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    assert!(
        flag_dir.join("config.toml").exists(),
        "flag-dir received the write"
    );
    assert!(
        !env_dir.join("config.toml").exists(),
        "env-dir was NOT used when flag is also set"
    );
}

#[test]
fn env_var_used_when_flag_absent() {
    let tmp = TempDir::new().unwrap();
    let env_dir = tmp.path().join("env-only");
    let repo = fixture_git_repo(tmp.path(), "myrepo");

    let mut cmd = assert_cmd::Command::cargo_bin("repograph").unwrap();
    cmd.env("REPOGRAPH_CONFIG_DIR", &env_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    assert!(env_dir.join("config.toml").exists());
}

#[test]
fn config_dir_flag_works_on_every_subcommand() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("conf");
    let repo = fixture_git_repo(tmp.path(), "myrepo");

    // add
    repograph_cmd_with_flag(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    // list
    let out = repograph_cmd_with_flag(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["name"].as_str(), Some("foo"));

    // remove
    repograph_cmd_with_flag(&config_dir)
        .arg("remove")
        .arg("foo")
        .assert()
        .success();
}
