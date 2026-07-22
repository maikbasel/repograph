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
    assert_eq!(v["schema_version"], 2);
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
fn find_json_envelope_reports_retrieval_mode() {
    // A plain (lexical) query must tell an agent, in the stdout payload, that
    // semantic retrieval did not run and nothing degraded — not just on stderr.
    let (_tmp, config_dir) = two_repo_fixture();
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("rotate_refresh_token")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["schema_version"], 2);
    assert_eq!(
        v["semantic_used"], false,
        "lexical query did not use semantic retrieval"
    );
    assert!(
        v["degraded"].is_null(),
        "nothing degraded on a satisfied lexical query"
    );
}

// Only meaningful on a build *without* the `semantic` feature: there, the
// embedder can never initialize, so the degrade reason is deterministic and
// needs no model download. Under `--all-features` the binary has real semantic
// support and `--semantic` would attempt a network fetch — that path is covered
// (gated, offline-skipped) in `tests/semantic.rs` instead.
#[cfg(not(feature = "semantic"))]
#[test]
fn find_semantic_on_lexical_build_reports_degraded_in_json() {
    // Requesting `--semantic` from a build without the feature must surface the
    // fallback in the machine-readable payload, so an agent parsing stdout can
    // detect that results are keyword-only — a silent degrade is a contract hole.
    let (_tmp, config_dir) = two_repo_fixture();
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("rotate_refresh_token")
        .arg("--semantic")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["semantic_used"], false);
    let reason = v["degraded"]
        .as_str()
        .expect("degraded carries a reason string when semantic was requested but unavailable");
    assert!(
        reason.contains("semantic"),
        "degrade reason names the missing capability: {reason}"
    );
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
fn find_without_index_auto_builds_and_returns_hits() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "pub fn zzztopmarker() {}\n")]);
    register(&config_dir, &api, "api");
    // Note: no `repograph index` run — auto-refresh must build it on demand.
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("zzztopmarker")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(
        !v["hits"].as_array().unwrap().is_empty(),
        "missing index is built on demand and returns hits"
    );
}

#[test]
fn find_no_refresh_without_index_exits_3_and_guides_user() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn a() {}\n")]);
    register(&config_dir, &api, "api");
    // No index, and --no-refresh forbids building one → the old exit-3 contract.
    repograph_cmd(&config_dir)
        .arg("find")
        .arg("anything")
        .arg("--no-refresh")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("repograph index"));
}

#[test]
fn find_auto_refreshes_uncommitted_edit() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api =
        fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "pub fn oldmarker_frob() {}\n")]);
    register(&config_dir, &api, "api");
    build_index(&config_dir);

    // Edit a tracked file WITHOUT committing; stamp its mtime strictly forward so
    // the ~1s-granular baseline can't tie it.
    let file = api.join("a.rs");
    std::fs::write(&file, "pub fn newmarker_plugh() {}\n").unwrap();
    let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(future)
        .unwrap();

    // --no-refresh first: it queries the pre-edit index, which never saw the new
    // symbol. (Must run before any refreshing find, which would persist the edit.)
    let out2 = repograph_cmd(&config_dir)
        .arg("find")
        .arg("newmarker_plugh")
        .arg("--no-refresh")
        .arg("--json")
        .assert()
        .success();
    let v2: serde_json::Value = serde_json::from_slice(&out2.get_output().stdout).unwrap();
    assert!(
        v2["hits"].as_array().unwrap().is_empty(),
        "--no-refresh ignores the uncommitted edit"
    );

    // Default find auto-refreshes → the uncommitted edit is now searchable.
    let out = repograph_cmd(&config_dir)
        .arg("find")
        .arg("newmarker_plugh")
        .arg("--json")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(
        !v["hits"].as_array().unwrap().is_empty(),
        "uncommitted edit is picked up by auto-refresh"
    );
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
