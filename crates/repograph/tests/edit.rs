//! Acceptance tests for `repograph edit`.
//!
//! Covers the `Edit updates a registered repository in place` scenarios from
//! the `registry-core` spec: in-place metadata change, not-found, usage error
//! on an empty edit, non-git path, rename-preserves-membership, and rename
//! conflict.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{fixture_git_repo, parse_repos_json, parse_workspace_show_json, repograph_cmd};

fn add_repo(config_dir: &std::path::Path, repo_path: &std::path::Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo_path)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

#[test]
fn edit_changes_description_and_stack_in_place() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    add_repo(&config_dir, &repo, "foo");

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--description")
        .arg("new")
        .arg("--stack")
        .arg("rust,cli")
        .assert()
        .success()
        .stderr(predicate::str::contains("foo"));

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["name"], "foo");
    assert_eq!(repos[0]["description"], "new");
    let stack: Vec<&str> = repos[0]["stack"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(stack, vec!["rust", "cli"]);
}

#[test]
fn edit_nonexistent_returns_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("ghost")
        .arg("--description")
        .arg("x")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

#[test]
fn edit_with_no_change_flags_is_usage_error() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    add_repo(&config_dir, &repo, "foo");

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .assert()
        .code(2);
}

#[test]
fn edit_with_only_empty_name_is_usage_error() {
    // An empty `--name` is not a rename (names must be non-empty), so an edit
    // carrying only `--name ""` changes nothing and must be a usage error
    // rather than a silent no-op write.
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    add_repo(&config_dir, &repo, "foo");

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--name")
        .arg("")
        .assert()
        .code(2);
}

#[test]
fn edit_empty_stack_clears_the_tags() {
    // `--stack ""` drops all tags, mirroring how `--description ""` clears the
    // description.
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    add_repo(&config_dir, &repo, "foo");

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--stack")
        .arg("rust,cli")
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--stack")
        .arg("")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert_eq!(repos.len(), 1);
    assert!(
        repos[0]["stack"].as_array().unwrap().is_empty(),
        "stack should be cleared, got: {}",
        repos[0]["stack"]
    );
}

#[test]
fn edit_to_non_git_path_returns_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    add_repo(&config_dir, &repo, "foo");

    let plain = tmp.path().join("plain-dir");
    std::fs::create_dir_all(&plain).unwrap();

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--path")
        .arg(&plain)
        .assert()
        .code(3);
}

#[test]
fn edit_rename_preserves_workspace_membership() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    add_repo(&config_dir, &repo, "foo");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("acme")
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("foo")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--name")
        .arg("bar")
        .assert()
        .success();

    // The entry is renamed in the registry.
    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["name"], "bar");

    // The workspace now lists `bar` as a live member, no dangling reference.
    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    let members = v["members"].as_array().expect("members array");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], "bar");
    assert!(
        v["dangling"].as_array().unwrap().is_empty(),
        "rename left a dangling member"
    );
}

#[test]
fn edit_rename_to_existing_name_returns_exit_5() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let foo = fixture_git_repo(tmp.path(), "foo");
    let bar = fixture_git_repo(tmp.path(), "bar");
    add_repo(&config_dir, &foo, "foo");
    add_repo(&config_dir, &bar, "bar");

    repograph_cmd(&config_dir)
        .arg("edit")
        .arg("foo")
        .arg("--name")
        .arg("bar")
        .assert()
        .code(5);
}
