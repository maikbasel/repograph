//! Acceptance tests for `--json` confirmation envelopes on mutating commands.
//!
//! Covers `Mutating registry commands emit a JSON confirmation envelope under
//! --json` (registry-core) and `Workspace mutating commands emit a JSON
//! confirmation envelope under --json` (workspace-support).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use tempfile::TempDir;

use crate::common::{fixture_git_repo, repograph_cmd};

fn json_stdout(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("stdout is a single JSON object")
}

#[test]
fn add_json_confirms_registered_entry() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "myrepo");

    let out = repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo)
        .arg("--name")
        .arg("foo")
        .arg("--stack")
        .arg("rust")
        .arg("--json")
        .assert()
        .success();
    let v = json_stdout(&out.get_output().stdout);
    assert_eq!(v["action"], "add");
    assert_eq!(v["repo"]["name"], "foo");
    assert_eq!(v["repo"]["stack"][0], "rust");
    assert!(
        v["repo"]["path"].as_str().unwrap().contains("myrepo"),
        "path echoed: {v}"
    );
}

#[test]
fn remove_json_confirms_removed_name() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    repograph_cmd(&config_dir)
        .args(["add"])
        .arg(&repo)
        .args(["--name", "foo"])
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .args(["remove", "foo", "--json"])
        .assert()
        .success();
    let v = json_stdout(&out.get_output().stdout);
    assert_eq!(v["action"], "remove");
    assert_eq!(v["name"], "foo");
}

#[test]
fn edit_json_confirms_updated_entry() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");
    repograph_cmd(&config_dir)
        .args(["add"])
        .arg(&repo)
        .args(["--name", "foo"])
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .args(["edit", "foo", "--description", "new", "--json"])
        .assert()
        .success();
    let v = json_stdout(&out.get_output().stdout);
    assert_eq!(v["action"], "edit");
    assert_eq!(v["repo"]["name"], "foo");
    assert_eq!(v["repo"]["description"], "new");
}

#[test]
fn workspace_create_json_confirms_workspace() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let out = repograph_cmd(&config_dir)
        .args(["workspace", "create", "acme", "--json"])
        .assert()
        .success();
    let v = json_stdout(&out.get_output().stdout);
    assert_eq!(v["action"], "workspace.create");
    assert_eq!(v["workspace"], "acme");
}

#[test]
fn workspace_add_json_confirms_members() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    let web = fixture_git_repo(tmp.path(), "web");
    repograph_cmd(&config_dir)
        .args(["add"])
        .arg(&api)
        .args(["--name", "api"])
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .args(["add"])
        .arg(&web)
        .args(["--name", "web"])
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .args(["workspace", "create", "acme"])
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .args(["workspace", "add", "acme", "api", "web", "--json"])
        .assert()
        .success();
    let v = json_stdout(&out.get_output().stdout);
    assert_eq!(v["action"], "workspace.add");
    assert_eq!(v["workspace"], "acme");
    let repos: Vec<&str> = v["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert!(repos.contains(&"api") && repos.contains(&"web"));
}

#[test]
fn workspace_rm_json_confirms_deletion() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    repograph_cmd(&config_dir)
        .args(["workspace", "create", "acme"])
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .args(["workspace", "rm", "acme", "--json"])
        .assert()
        .success();
    let v = json_stdout(&out.get_output().stdout);
    assert_eq!(v["action"], "workspace.rm");
    assert_eq!(v["workspace"], "acme");
}

#[test]
fn add_without_json_keeps_stdout_empty() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "foo");

    let out = repograph_cmd(&config_dir)
        .args(["add"])
        .arg(&repo)
        .args(["--name", "foo"])
        .assert()
        .success();
    assert!(
        out.get_output().stdout.is_empty(),
        "stdout must be empty without --json, got: {:?}",
        String::from_utf8_lossy(&out.get_output().stdout)
    );
    // Confirmation is on stderr instead.
    assert!(String::from_utf8_lossy(&out.get_output().stderr).contains("foo"));
}

#[test]
fn workspace_create_without_json_keeps_stdout_empty() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let out = repograph_cmd(&config_dir)
        .args(["workspace", "create", "acme"])
        .assert()
        .success();
    assert!(
        out.get_output().stdout.is_empty(),
        "stdout must be empty without --json"
    );
}
