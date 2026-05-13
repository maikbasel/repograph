//! Acceptance tests for `repograph workspace …` and `repograph list --workspace`.
//!
//! Mirrors the structure of `tests/list.rs` and `tests/remove.rs`: every spec
//! scenario from `openspec/changes/workspace-support/specs/workspace-support/spec.md`
//! has at least one test here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{
    fixture_git_repo, parse_list_json, parse_repos_json, parse_workspace_show_json,
    parse_workspaces_json, repograph_cmd,
};

fn add_repo(config_dir: &std::path::Path, repo_path: &std::path::Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo_path)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

fn create_workspace(config_dir: &std::path::Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("workspace")
        .arg("create")
        .arg(name)
        .assert()
        .success();
}

// ─── workspace create ──────────────────────────────────────────────────────

#[test]
fn create_with_explicit_description_persists_to_config() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("acme")
        .arg("--description")
        .arg("Acme rebuild")
        .assert()
        .success()
        .stderr(predicate::str::contains("acme"));

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(body.contains("[workspace.acme]"), "got: {body}");
    assert!(body.contains("Acme rebuild"), "description persisted");
}

#[test]
fn create_without_description() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("acme")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(body.contains("[workspace.acme]"));
    assert!(!body.contains("description"), "no description key when absent: {body}");
}

#[test]
fn create_name_conflict_returns_exit_5() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    create_workspace(&config_dir, "acme");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("acme")
        .assert()
        .code(5)
        .stderr(predicate::str::contains("acme"));
}

#[test]
fn create_invalid_name_returns_exit_2() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("Acme")
        .assert()
        .code(2);

    // Config file must not exist after a rejected create.
    assert!(!config_dir.join("config.toml").exists());
}

#[test]
fn create_leading_hyphen_rejected() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("-acme")
        .assert()
        .code(2);
}

#[test]
fn create_empty_name_rejected() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("")
        .assert()
        .code(2);
}

#[test]
fn create_overlength_name_rejected() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let too_long = "a".repeat(64);
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg(&too_long)
        .assert()
        .code(2);
}

#[test]
fn create_reserved_name_rejected() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    for reserved in &["default", "all", "none"] {
        repograph_cmd(&config_dir)
            .arg("workspace")
            .arg("create")
            .arg(reserved)
            .assert()
            .code(2);
    }
}

// ─── workspace rm ──────────────────────────────────────────────────────────

#[test]
fn rm_existing_workspace() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("rm")
        .arg("acme")
        .assert()
        .success()
        .stderr(predicate::str::contains("acme"));

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(!body.contains("[workspace.acme]"));
    // Repo is untouched.
    assert!(body.contains("[repo.api]"), "repo entry preserved: {body}");
}

#[test]
fn rm_nonexistent_workspace_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("rm")
        .arg("ghost")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

// ─── workspace ls ──────────────────────────────────────────────────────────

#[test]
fn ls_json_envelope_shape() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    create_workspace(&config_dir, "alpha");
    create_workspace(&config_dir, "beta");

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("ls")
        .arg("--json")
        .assert()
        .success();
    let ws = parse_workspaces_json(&out.get_output().stdout);
    assert_eq!(ws.len(), 2);
    assert_eq!(ws[0]["name"], "alpha");
    assert_eq!(ws[1]["name"], "beta");
}

#[test]
fn ls_empty_emits_empty_array() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("ls")
        .arg("--json")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(stdout.trim_end().contains("\"workspaces\":[]"), "got: {stdout}");
}

#[test]
fn ls_orders_alphabetically_and_stably() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    create_workspace(&config_dir, "zeta");
    create_workspace(&config_dir, "alpha");
    create_workspace(&config_dir, "mid");

    for _ in 0..2 {
        let out = repograph_cmd(&config_dir)
            .arg("workspace")
            .arg("ls")
            .arg("--json")
            .assert()
            .success();
        let ws = parse_workspaces_json(&out.get_output().stdout);
        let names: Vec<&str> = ws.iter().map(|w| w["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
    }
}

#[test]
fn ls_when_piped_emits_json() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    create_workspace(&config_dir, "acme");

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("ls")
        .assert()
        .success();
    // assert_cmd pipes stdout → non-TTY → JSON mode.
    let value: serde_json::Value =
        serde_json::from_slice(&out.get_output().stdout).expect("clean JSON");
    assert!(value["workspaces"].is_array());
}

// ─── workspace show ────────────────────────────────────────────────────────

#[test]
fn show_json_envelope_lists_live_members_with_empty_dangling() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ui");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ui");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ui")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    assert_eq!(v["name"], "acme");
    let members = v["members"].as_array().expect("members array");
    assert_eq!(members.len(), 2);
    let names: Vec<&str> = members.iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"api"));
    assert!(names.contains(&"ui"));
    let dangling = v["dangling"].as_array().expect("dangling array always present");
    assert!(dangling.is_empty());
}

#[test]
fn show_with_dangling_member_separates_live_and_tombstoned() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ghost");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ghost");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ghost")
        .assert()
        .success();

    // Tombstone: deregister `ghost`, leaving workspace membership intact.
    repograph_cmd(&config_dir)
        .arg("remove")
        .arg("ghost")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    let members = v["members"].as_array().unwrap();
    assert_eq!(members.len(), 1, "only api is live");
    assert_eq!(members[0]["name"], "api");
    let dangling = v["dangling"].as_array().unwrap();
    assert_eq!(dangling.len(), 1);
    assert_eq!(dangling[0], "ghost");

    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("ghost"), "stderr warns about dangling: {stderr}");
}

#[test]
fn show_nonexistent_workspace_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("ghost")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

// ─── workspace add ─────────────────────────────────────────────────────────

#[test]
fn add_single_repo() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    let members = v["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], "api");
}

#[test]
fn add_multiple_repos_sorted() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ui");
    let r3 = fixture_git_repo(tmp.path(), "libs");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ui");
    add_repo(&config_dir, &r3, "libs");
    create_workspace(&config_dir, "acme");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("ui")
        .arg("api")
        .arg("libs")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    // members = ["api", "libs", "ui"]
    let acme_idx = body.find("[workspace.acme]").expect("workspace section");
    let after = &body[acme_idx..];
    let api_pos = after.find("api").expect("api present");
    let libs_pos = after.find("libs").expect("libs present");
    let ui_pos = after.find("ui").expect("ui present");
    assert!(api_pos < libs_pos && libs_pos < ui_pos, "members sorted: {after}");
}

#[test]
fn add_idempotent_for_existing_member() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    assert_eq!(v["members"].as_array().unwrap().len(), 1);
}

#[test]
fn add_missing_workspace_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("ghost")
        .arg("api")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

#[test]
fn add_missing_repo_is_atomic() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ui");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ui");
    create_workspace(&config_dir, "acme");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ghost")
        .arg("ui")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    assert!(v["members"].as_array().unwrap().is_empty(), "no partial application");
}

// ─── workspace remove ──────────────────────────────────────────────────────

#[test]
fn remove_single_member_keeps_repo_registered() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ui");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ui");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ui")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("remove")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    // api is still registered.
    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(body.contains("[repo.api]"), "repo preserved: {body}");

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    let names: Vec<&str> = v["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["ui"]);
}

#[test]
fn remove_non_member_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("remove")
        .arg("acme")
        .arg("ghost")
        .assert()
        .success();
}

#[test]
fn remove_missing_workspace_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("remove")
        .arg("ghost")
        .arg("api")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

// ─── list --workspace filter ────────────────────────────────────────────────

#[test]
fn list_filtered_by_workspace_json() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ui");
    let r3 = fixture_git_repo(tmp.path(), "libs");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ui");
    add_repo(&config_dir, &r3, "libs");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ui")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--workspace")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    let names: Vec<&str> = repos.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["api", "ui"]);
}

#[test]
fn list_filtered_by_workspace_skips_dangling() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ghost");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ghost");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ghost")
        .assert()
        .success();

    repograph_cmd(&config_dir).arg("remove").arg("ghost").assert().success();

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--workspace")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    let names: Vec<&str> = repos.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["api"], "ghost silently skipped");

    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        !stderr.to_lowercase().contains("dangling"),
        "list does NOT emit a dangling warning: {stderr}"
    );
}

#[test]
fn list_filtered_by_nonexistent_workspace_exit_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("list")
        .arg("--workspace")
        .arg("ghost")
        .arg("--json")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

#[test]
fn list_without_workspace_flag_unchanged() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "alone");
    add_repo(&config_dir, &r, "alone");
    create_workspace(&config_dir, "acme");

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--json")
        .assert()
        .success();
    let repos = parse_repos_json(&out.get_output().stdout);
    assert_eq!(repos.len(), 1, "all registered repos appear");
}

// ─── tombstone semantics & registry-core invariance ────────────────────────

#[test]
fn repo_remove_leaves_workspace_member_intact() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    repograph_cmd(&config_dir).arg("remove").arg("api").assert().success();

    // Members list still has `api`.
    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(body.contains("members"));
    assert!(body.contains("\"api\""), "dangling member preserved: {body}");
    assert!(!body.contains("[repo.api]"), "repo deregistered: {body}");
}

#[test]
fn registry_remove_behavior_unchanged_with_workspace_membership() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("remove")
        .arg("api")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(stdout.trim().is_empty(), "stdout empty just like registry-core");
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    // Same stderr message shape as the registry-core tests (contains the name).
    assert!(stderr.contains("api"));
    // No "workspace" or "dangling" chatter from the registry-core path.
    let lower = stderr.to_lowercase();
    assert!(
        !lower.contains("workspace") && !lower.contains("dangling"),
        "registry-core remove is unaware of workspaces: {stderr}"
    );
}

#[test]
fn dangling_member_re_registers_cleanly() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    // Remove & confirm dangling.
    repograph_cmd(&config_dir).arg("remove").arg("api").assert().success();
    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    assert_eq!(v["dangling"].as_array().unwrap().len(), 1);

    // Re-register under same name; workspace heals on next read.
    add_repo(&config_dir, &r, "api");
    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let v = parse_workspace_show_json(&out.get_output().stdout);
    assert_eq!(v["dangling"].as_array().unwrap().len(), 0);
    assert_eq!(v["members"].as_array().unwrap().len(), 1);
}

// ─── output contract & round-trip ──────────────────────────────────────────

#[test]
fn workspace_show_dangling_warning_on_stderr_only() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ghost");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ghost");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ghost")
        .assert()
        .success();
    repograph_cmd(&config_dir).arg("remove").arg("ghost").assert().success();

    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("show")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    // stdout is pure JSON.
    let _: serde_json::Value =
        serde_json::from_slice(&out.get_output().stdout).expect("clean JSON on stdout");
    // stderr carries the dangling warning.
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("ghost"));
}

#[test]
fn workspace_create_confirmation_on_stderr_only() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("create")
        .arg("acme")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(stdout.trim().is_empty(), "stdout empty for create: {stdout:?}");
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("acme"));
}

#[test]
fn mixed_repo_and_workspace_round_trip_is_stable() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r1 = fixture_git_repo(tmp.path(), "api");
    let r2 = fixture_git_repo(tmp.path(), "ui");
    add_repo(&config_dir, &r1, "api");
    add_repo(&config_dir, &r2, "ui");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .arg("ui")
        .assert()
        .success();

    let body_before = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    // Trigger a re-save without other mutation: add then remove the same member.
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();
    let body_after = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert_eq!(body_before, body_after, "no-op write is byte-stable");
}

#[test]
fn workspace_add_with_no_repos_is_usage_error() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    create_workspace(&config_dir, "acme");

    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .assert()
        .code(2);
}

#[test]
fn workspace_subcommand_help_lists_all_verbs() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let out = repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("--help")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    for verb in &["create", "rm", "ls", "show", "add", "remove"] {
        assert!(stdout.contains(verb), "help mentions `{verb}`: {stdout}");
    }
}

// ─── ls envelope also confirms list envelope shape for repos ───────────────

#[test]
fn list_envelope_contains_repos_key_even_when_filtered() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let r = fixture_git_repo(tmp.path(), "api");
    add_repo(&config_dir, &r, "api");
    create_workspace(&config_dir, "acme");
    repograph_cmd(&config_dir)
        .arg("workspace")
        .arg("add")
        .arg("acme")
        .arg("api")
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("list")
        .arg("--workspace")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let envelope = parse_list_json(&out.get_output().stdout);
    assert!(envelope.get("repos").is_some(), "envelope shape: {envelope}");
}
