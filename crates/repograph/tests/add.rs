//! Acceptance tests for `repograph add`.
//!
//! Covers every `Scenario` under spec `Add registers a local git repository`
//! plus `Path stored as canonical absolute` and `Description and stack metadata`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{fixture_git_repo, repograph_cmd};

#[test]
fn successful_add_with_explicit_name() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo_path = fixture_git_repo(tmp.path(), "myrepo");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo_path)
        .arg("--name")
        .arg("foo")
        .assert()
        .success()
        .stderr(predicate::str::contains("foo"));

    let toml_path = config_dir.join("config.toml");
    assert!(toml_path.exists(), "config file written");
    let body = std::fs::read_to_string(&toml_path).unwrap();
    assert!(
        body.contains("[repo.foo]"),
        "config contains [repo.foo] entry, got:\n{body}"
    );
}

#[test]
fn add_infers_name_from_path_basename() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo_path = fixture_git_repo(tmp.path(), "inferred-name");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo_path)
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(
        body.contains("[repo.inferred-name]"),
        "name inferred from basename, got:\n{body}"
    );
}

#[test]
fn path_stored_as_canonical_absolute() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let canonical = fixture_git_repo(tmp.path(), "cano");

    // Pass a path with `..` segments that resolves to the same place.
    let messy = tmp.path().join("cano").join(".").join(".");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&messy)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    let canonical_str = canonical.to_string_lossy();
    assert!(
        body.contains(canonical_str.as_ref()),
        "stored path is canonical {canonical_str}, got:\n{body}"
    );
}

#[test]
fn description_and_stack_metadata_persist() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo_path = fixture_git_repo(tmp.path(), "metarepo");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo_path)
        .arg("--name")
        .arg("foo")
        .arg("--description")
        .arg("hello world")
        .arg("--stack")
        .arg("rust,cli")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(
        body.contains("description = \"hello world\""),
        "description persisted, got:\n{body}"
    );
    assert!(
        body.contains("\"rust\"") && body.contains("\"cli\""),
        "stack persisted, got:\n{body}"
    );
}

#[test]
fn add_rejects_non_git_directory_with_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let plain_dir = tmp.path().join("plain");
    std::fs::create_dir(&plain_dir).unwrap();

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&plain_dir)
        .arg("--name")
        .arg("foo")
        .assert()
        .code(3)
        .stderr(predicate::str::is_empty().not());

    assert!(
        !config_dir.join("config.toml").exists(),
        "no config file written on rejection"
    );
}

#[test]
fn add_rejects_nonexistent_path_with_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let missing = tmp.path().join("nonexistent");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&missing)
        .arg("--name")
        .arg("foo")
        .assert()
        .code(3)
        .stderr(predicate::str::is_empty().not());

    assert!(
        !config_dir.join("config.toml").exists(),
        "no config file written on rejection"
    );
}

#[test]
fn add_name_conflict_returns_exit_5() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let first = fixture_git_repo(tmp.path(), "first");
    let second = fixture_git_repo(tmp.path(), "second");

    // First add succeeds.
    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&first)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    // Second add with same name → exit 5.
    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&second)
        .arg("--name")
        .arg("foo")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("foo"));

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    let first_canon = std::fs::canonicalize(&first).unwrap();
    let second_canon = std::fs::canonicalize(&second).unwrap();
    assert!(
        body.contains(first_canon.to_string_lossy().as_ref()),
        "original entry still present"
    );
    assert!(
        !body.contains(second_canon.to_string_lossy().as_ref()),
        "conflicting entry not added"
    );
}

#[test]
fn add_path_conflict_returns_exit_5() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "shared");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("foo")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("bar")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("foo"));

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(body.contains("[repo.foo]"), "original name preserved");
    assert!(
        !body.contains("[repo.bar]"),
        "conflicting name not added, got:\n{body}"
    );
}
