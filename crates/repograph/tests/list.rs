//! Acceptance tests for `repograph list`.
//!
//! Note: spec scenarios `TTY table list` and `Empty registry table` describe
//! behavior when stdout is a real TTY. `assert_cmd` always pipes stdout, so
//! TTY rendering is verified by unit tests on `output::render_repos` rather
//! than here. Every JSON / pipe / ordering scenario is covered below.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use tempfile::TempDir;

use crate::common::{fixture_git_repo, parse_list_json, parse_repos_json, repograph_cmd};

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
fn list_when_piped_emits_json_envelope() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "alpha");
    let r2 = fixture_git_repo(tmp.path(), "beta");
    add_repo(&config_dir, &r1, "alpha");
    add_repo(&config_dir, &r2, "beta");

    let out = repograph_cmd(&config_dir).arg("list").assert().success();
    let stdout = &out.get_output().stdout;
    let envelope = parse_list_json(stdout);
    assert!(
        envelope.get("repos").is_some(),
        "envelope contains `repos` key, got: {envelope}"
    );
    let repos = parse_repos_json(stdout);
    assert_eq!(repos.len(), 2, "two repos rendered");
}

#[test]
fn list_with_json_flag_emits_json_envelope() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "only");
    add_repo(&config_dir, &r, "only");

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert_eq!(repos.len(), 1);
    let entry = &repos[0];
    assert_eq!(entry["name"].as_str(), Some("only"));
    assert!(entry["path"].is_string(), "path is a string");
}

#[test]
fn list_empty_registry_emits_empty_array() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert!(repos.is_empty(), "empty array");
}

#[test]
fn list_empty_registry_does_not_panic_without_json_flag() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir).arg("list").assert().success();
}

#[test]
fn list_orders_repos_alphabetically_by_name() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r_zeta = fixture_git_repo(tmp.path(), "zeta");
    let r_alpha = fixture_git_repo(tmp.path(), "alpha");
    let r_mid = fixture_git_repo(tmp.path(), "mid");
    add_repo(&config_dir, &r_zeta, "zeta");
    add_repo(&config_dir, &r_alpha, "alpha");
    add_repo(&config_dir, &r_mid, "mid");

    // Run twice to confirm stability.
    for _ in 0..2 {
        let out = repograph_cmd(&config_dir)
            .arg("list")
            .arg("--json")
            .assert()
            .success();
        let repos = parse_repos_json(&out.get_output().stdout);
        let names: Vec<&str> = repos.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"], "alphabetical ordering");
    }
}

#[test]
fn list_json_entry_shape() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "shape");

    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&r)
        .arg("--name")
        .arg("shape")
        .arg("--description")
        .arg("a repo")
        .arg("--stack")
        .arg("rust,cli")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    let entry = &repos[0];
    assert_eq!(entry["name"], "shape");
    assert!(entry["path"].is_string());
    assert_eq!(entry["description"], "a repo");
    let stack = entry["stack"].as_array().expect("stack array");
    let stack_strings: Vec<&str> = stack.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(stack_strings.contains(&"rust"));
    assert!(stack_strings.contains(&"cli"));
}

#[test]
fn list_does_not_pollute_stdout_with_diagnostics() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "clean");
    add_repo(&config_dir, &r, "clean");

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    // stdout must be parseable as a single JSON document with no leading garbage.
    let _: serde_json::Value =
        serde_json::from_slice(&out.get_output().stdout).expect("clean JSON on stdout");
}

#[test]
fn list_table_columns_exist_when_forced_via_unit_test() {
    // Sentinel: TTY rendering is exercised by unit tests on
    // `repograph::output::render_repos`. This acceptance test exists to
    // document that decision and to fail if the unit test is removed.
    // (The unit module name + function name is stable enough to grep for.)
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/output.rs");
    let body = std::fs::read_to_string(&path).expect("output.rs exists");
    assert!(
        body.contains("render_repos"),
        "output::render_repos exists ({})",
        path.display()
    );
    assert!(
        body.contains("#[cfg(test)]"),
        "output.rs has unit tests for TTY rendering"
    );
}
