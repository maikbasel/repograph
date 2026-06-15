//! Acceptance tests for `repograph index`.
//!
//! Covers the `cross-repo-search` spec scenarios for building and incrementally
//! refreshing the index. Output is on stderr (the index is a side effect); these
//! tests assert exit codes and observable effects via `repograph find`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{commit_files, fixture_git_repo_with_files, repograph_cmd};

fn register(config_dir: &Path, repo: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

fn find_json(config_dir: &Path, query: &str) -> serde_json::Value {
    let out = repograph_cmd(config_dir)
        .arg("find")
        .arg(query)
        .arg("--json")
        .assert()
        .success();
    serde_json::from_slice(&out.get_output().stdout).expect("find stdout is JSON")
}

#[test]
fn index_populates_and_excludes_untracked() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(
        tmp.path(),
        "api",
        &[("auth.rs", "fn rotate_refresh_token() { let _ = 1; }\n")],
    );
    // An untracked file must not be indexed.
    std::fs::write(api.join("untracked.rs"), "fn untracked_secret() {}\n").unwrap();
    register(&config_dir, &api, "api");

    repograph_cmd(&config_dir).arg("index").assert().success();

    let tracked = find_json(&config_dir, "rotate_refresh_token");
    assert!(
        !tracked["hits"].as_array().unwrap().is_empty(),
        "tracked content is indexed"
    );

    let untracked = find_json(&config_dir, "untracked_secret");
    assert!(
        untracked["hits"].as_array().unwrap().is_empty(),
        "untracked file is excluded from the index"
    );
}

#[test]
fn index_empty_registry_is_exit_0_with_notice() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    repograph_cmd(&config_dir)
        .arg("index")
        .assert()
        .success()
        .stderr(predicate::str::contains("Nothing to index"));
}

#[test]
fn reindex_unchanged_reports_up_to_date() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn alpha() {}\n")]);
    register(&config_dir, &api, "api");

    repograph_cmd(&config_dir).arg("index").assert().success();
    // Second run with no changes.
    repograph_cmd(&config_dir)
        .arg("index")
        .assert()
        .success()
        .stderr(predicate::str::contains("up to date"));
}

#[test]
fn reindex_after_change_reflects_new_content_and_purges_old() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    // Distinct single-token identifiers so neither query can match the other
    // via a shared sub-token (the lexical arm ORs query tokens for recall).
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn alphafunc() {}\n")]);
    register(&config_dir, &api, "api");
    repograph_cmd(&config_dir).arg("index").assert().success();
    assert!(
        !find_json(&config_dir, "alphafunc")["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Replace the file's content and re-index.
    commit_files(&api, "rename", &[("a.rs", "fn betafunc() {}\n")]);
    repograph_cmd(&config_dir).arg("index").assert().success();

    assert!(
        !find_json(&config_dir, "betafunc")["hits"]
            .as_array()
            .unwrap()
            .is_empty(),
        "new content is searchable"
    );
    assert!(
        find_json(&config_dir, "alphafunc")["hits"]
            .as_array()
            .unwrap()
            .is_empty(),
        "stale content is purged"
    );
}

#[test]
fn index_scoped_to_workspace_excludes_other_repos() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn alphaonly() {}\n")]);
    let ui = fixture_git_repo_with_files(tmp.path(), "ui", &[("b.rs", "fn betaonly() {}\n")]);
    register(&config_dir, &api, "api");
    register(&config_dir, &ui, "ui");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("backend")
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("backend")
        .arg("api")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("index")
        .arg("--workspace")
        .arg("backend")
        .assert()
        .success();

    assert!(
        !find_json(&config_dir, "alphaonly")["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        find_json(&config_dir, "betaonly")["hits"]
            .as_array()
            .unwrap()
            .is_empty(),
        "repo outside the workspace was not indexed"
    );
}
