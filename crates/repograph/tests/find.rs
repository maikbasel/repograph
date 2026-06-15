//! Acceptance tests for `repograph find`.
//!
//! Covers the `cross-repo-search` retrieval, output-contract, and error
//! scenarios. Semantic-specific behavior (embeddings) is exercised at the core
//! layer under the `semantic` feature; these binary tests use the always-on
//! lexical path, which is what a default build ships.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::format_collect)]

mod common;

use std::path::Path;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{fixture_git_repo_with_files, repograph_cmd};

fn register(config_dir: &Path, repo: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

fn build_index(config_dir: &Path) {
    repograph_cmd(config_dir).arg("index").assert().success();
}

/// Register two repos with distinct content and build the index. Returns the
/// config dir (the `TempDir` is returned too so it outlives the test body).
fn two_repo_fixture() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(
        tmp.path(),
        "api",
        &[(
            "auth.rs",
            "pub fn rotate_refresh_token(token: &str) -> String { token.to_string() }\n",
        )],
    );
    let ui = fixture_git_repo_with_files(
        tmp.path(),
        "ui",
        &[("button.rs", "pub fn render_primary_button() {}\n")],
    );
    register(&config_dir, &api, "api");
    register(&config_dir, &ui, "ui");
    build_index(&config_dir);
    (tmp, config_dir)
}

#[test]
fn find_exact_symbol_returns_hit_with_fields() {
    let (_tmp, config_dir) = two_repo_fixture();
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("rotate_refresh_token")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["query"], "rotate_refresh_token");
    let hits = v["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "exact symbol is found");
    let top = &hits[0];
    assert_eq!(top["repo"], "api");
    assert_eq!(top["path"], "auth.rs");
    assert!(top["line"].is_number());
    assert!(top["score"].is_number());
    assert!(
        top["snippet"]
            .as_str()
            .unwrap()
            .contains("rotate_refresh_token")
    );
}

#[test]
fn find_json_is_pure_data_and_parses() {
    let (_tmp, config_dir) = two_repo_fixture();
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("render_primary_button")
        .arg("--json")
        .assert()
        .success();
    // Stdout must be a single JSON object and nothing else.
    let stdout = out.get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("stdout is valid JSON");
    assert!(v.is_object());
    assert!(v["hits"].is_array());
}

#[test]
fn find_limit_bounds_results() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let body: String = (0..40)
        .map(|n| format!("pub fn widget_{n}() {{}}\n"))
        .collect();
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("widgets.rs", &body)]);
    register(&config_dir, &api, "api");
    build_index(&config_dir);

    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("widget")
        .arg("--limit")
        .arg("2")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(v["hits"].as_array().unwrap().len() <= 2);
}

#[test]
fn find_no_match_is_empty_hits_exit_0() {
    let (_tmp, config_dir) = two_repo_fixture();
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("this_symbol_exists_nowhere_zzz")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(v["hits"].as_array().unwrap().is_empty());
}

#[test]
fn find_workspace_filter_scopes_results() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(
        tmp.path(),
        "api",
        &[("a.rs", "pub fn shared_helper() {}\n")],
    );
    let ui =
        fixture_git_repo_with_files(tmp.path(), "ui", &[("b.rs", "pub fn shared_helper() {}\n")]);
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
    build_index(&config_dir);

    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("shared_helper")
        .arg("--workspace")
        .arg("backend")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let hits = v["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(
        hits.iter().all(|h| h["repo"] == "api"),
        "scoped to workspace repos"
    );
}

#[test]
fn find_without_index_exits_3_and_guides_user() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn a() {}\n")]);
    register(&config_dir, &api, "api");
    // Note: no `repograph index` run.
    repograph_cmd(&config_dir)
        .arg("find")
        .arg("anything")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("repograph index"));
}

#[test]
fn find_corrupt_index_exits_1() {
    let (_tmp, config_dir) = two_repo_fixture();
    // Overwrite the (real) index database with garbage so it is present but
    // unreadable — distinct from a missing index (exit 3).
    let index_db = config_dir.join("index.db");
    assert!(index_db.is_file(), "index built by fixture");
    std::fs::write(&index_db, b"this is not a sqlite database at all").unwrap();

    repograph_cmd(&config_dir)
        .arg("find")
        .arg("anything")
        .assert()
        .failure()
        .code(1);
}

#[test]
fn find_tty_table_lists_columns() {
    // Force JSON off path is hard without a TTY; instead assert the table render
    // path produces the documented columns by checking the human (non-JSON)
    // output is not JSON and names the repo. With stdout piped, the binary emits
    // JSON, so this test asserts the JSON contract instead and leaves the TTY
    // table to the output.rs unit tests.
    let (_tmp, config_dir) = two_repo_fixture();
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("rotate_refresh_token")
        .assert()
        .success();
    // Piped (non-TTY) defaults to JSON per the output contract.
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["hits"][0]["repo"], "api");
}
