//! Acceptance tests for `repograph remove`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{fixture_git_repo, parse_repos_json, repograph_cmd};

#[test]
fn successful_remove() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("remove")
        .arg("foo")
        .assert()
        .success()
        .stderr(predicate::str::contains("foo"));

    // Verify it's gone.
    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert!(repos.is_empty(), "registry empty after remove");
}

#[test]
fn remove_nonexistent_returns_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("remove")
        .arg("ghost")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}
