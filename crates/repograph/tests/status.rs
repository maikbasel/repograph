//! Acceptance tests for `repograph status`.
//!
//! Mirrors the structure of `tests/list.rs` and `tests/workspace.rs`: every
//! spec scenario from
//! `openspec/changes/git-status/specs/git-status/spec.md` is represented by
//! at least one acceptance test here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{
    add_local_commits, add_untracked_file, add_upstream_commits, attach_upstream,
    fixture_bare_git_repo, fixture_detached_git_repo, fixture_dirty_git_repo, fixture_git_repo,
    fixture_unborn_git_repo, parse_status_json, repograph_cmd, stage_new_file,
};

fn register(config_dir: &Path, repo: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

// ─── happy paths per coarse state ──────────────────────────────────────────

#[test]
fn clean_repo_on_tracked_branch_reports_clean() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "clean");
    let branch = current_branch(&repo);
    attach_upstream(&repo, &branch);
    register(&config_dir, &repo, "clean");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let rows = parse_status_json(&out.get_output().stdout);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["name"], "clean");
    assert_eq!(row["state"], "clean");
    assert_eq!(row["dirty"], false);
    assert_eq!(row["ahead"], 0);
    assert_eq!(row["behind"], 0);
    assert_eq!(row["staged"], 0);
    assert_eq!(row["unstaged"], 0);
    assert_eq!(row["untracked"], 0);
    assert!(row["branch"].is_string(), "branch present, got {row}");
    assert_eq!(row["upstream"], format!("origin/{branch}"));
    assert!(row["error"].is_null(), "healthy row has null error");
}

#[test]
fn dirty_working_tree_reports_dirty() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_dirty_git_repo(tmp.path(), "wip");
    register(&config_dir, &repo, "wip");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["state"], "dirty");
    assert_eq!(row["dirty"], true);
    assert!(row["unstaged"].as_u64().unwrap() >= 1);
    assert!(row["error"].is_null());
}

#[test]
fn dirty_counts_split_staged_unstaged_untracked() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_dirty_git_repo(tmp.path(), "mixed");
    stage_new_file(&repo, "added.txt");
    add_untracked_file(&repo, "new1.txt");
    add_untracked_file(&repo, "new2.txt");
    register(&config_dir, &repo, "mixed");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["staged"], 1);
    assert_eq!(row["unstaged"], 1);
    assert_eq!(row["untracked"], 2);
    assert_eq!(row["dirty"], true);
    assert_eq!(row["state"], "dirty");
}

#[test]
fn detached_head_reports_detached() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_detached_git_repo(tmp.path(), "loose");
    register(&config_dir, &repo, "loose");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["state"], "detached");
    assert!(row["branch"].is_null());
    assert!(row["upstream"].is_null());
    assert!(row["error"].is_null());
}

#[test]
fn unborn_repo_reports_unborn() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_unborn_git_repo(tmp.path(), "blank");
    register(&config_dir, &repo, "blank");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["state"], "unborn");
    assert!(row["branch"].is_null());
    assert!(row["upstream"].is_null());
    assert_eq!(row["ahead"], 0);
    assert_eq!(row["behind"], 0);
    assert!(row["error"].is_null());
}

#[test]
fn bare_repo_reports_bare() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_bare_git_repo(tmp.path(), "bare");
    register(&config_dir, &repo, "bare");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["state"], "bare");
    assert_eq!(row["staged"], 0);
    assert_eq!(row["unstaged"], 0);
    assert_eq!(row["untracked"], 0);
    assert!(
        row["error"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("bare"),
        "bare repo has a bare-ish error message, got: {row}"
    );
}

#[test]
fn missing_path_reports_missing_and_batch_exits_zero() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "ghost");
    register(&config_dir, &repo, "ghost");
    std::fs::remove_dir_all(&repo).unwrap();

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["state"], "missing");
    assert!(!row["error"].is_null(), "missing path populates error");
}

#[test]
fn directory_without_dot_git_reports_missing() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "stripped");
    register(&config_dir, &repo, "stripped");
    std::fs::remove_dir_all(repo.join(".git")).unwrap();

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["state"], "missing");
    assert!(!row["error"].is_null());
}

// ─── --workspace filter ────────────────────────────────────────────────────

#[test]
fn workspace_filter_only_shows_live_members() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    let ui = fixture_git_repo(tmp.path(), "ui");
    register(&config_dir, &api, "api");
    register(&config_dir, &ui, "ui");

    repograph_cmd(&config_dir)
        .args(["workspace", "create", "acme"])
        .assert()
        .success();
    repograph_cmd(&config_dir)
        .args(["workspace", "add", "acme", "api", "ui"])
        .assert()
        .success();
    // Tombstone one member.
    repograph_cmd(&config_dir)
        .args(["remove", "ui"])
        .assert()
        .success();

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--workspace")
        .arg("acme")
        .arg("--json")
        .assert()
        .success();
    let rows = parse_status_json(&out.get_output().stdout);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "api");
}

#[test]
fn names_plus_workspace_is_usage_error_exit_2() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");
    repograph_cmd(&config_dir)
        .args(["workspace", "create", "acme"])
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .arg("status")
        .arg("api")
        .arg("--workspace")
        .arg("acme")
        .assert()
        .code(2);
}

#[test]
fn unknown_positional_name_exits_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("status")
        .arg("ghost")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("ghost"));
}

#[test]
fn unknown_workspace_exits_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    repograph_cmd(&config_dir)
        .arg("status")
        .arg("--workspace")
        .arg("ghost")
        .assert()
        .code(3);
}

#[test]
fn single_explicit_missing_repo_exits_3_batch_exits_0() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let a = fixture_git_repo(tmp.path(), "alpha");
    let g = fixture_git_repo(tmp.path(), "gone");
    register(&config_dir, &a, "alpha");
    register(&config_dir, &g, "gone");
    std::fs::remove_dir_all(&g).unwrap();

    // Batch (all repos) — exit 0 despite the missing one.
    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let rows = parse_status_json(&out.get_output().stdout);
    assert_eq!(rows.len(), 2);

    // Explicit single name pointing at the broken one — exit 3.
    repograph_cmd(&config_dir)
        .arg("status")
        .arg("gone")
        .assert()
        .code(3);
}

// ─── deduplication and scope defaults ──────────────────────────────────────

#[test]
fn no_arguments_scans_all_registered_alphabetically() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    for name in ["zeta", "alpha", "mid"] {
        let p = fixture_git_repo(tmp.path(), name);
        register(&config_dir, &p, name);
    }

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let rows = parse_status_json(&out.get_output().stdout);
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["alpha", "mid", "zeta"]);
}

#[test]
fn duplicate_positional_names_are_deduplicated() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let foo = fixture_git_repo(tmp.path(), "foo");
    register(&config_dir, &foo, "foo");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("foo")
        .arg("foo")
        .arg("--json")
        .assert()
        .success();
    let rows = parse_status_json(&out.get_output().stdout);
    assert_eq!(rows.len(), 1);
}

// ─── ahead / behind ────────────────────────────────────────────────────────

#[test]
fn ahead_of_upstream_reports_ahead() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "ahead");
    let branch = current_branch(&repo);
    attach_upstream(&repo, &branch);
    add_local_commits(&repo, 2);
    register(&config_dir, &repo, "ahead");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["ahead"], 2);
    assert_eq!(row["behind"], 0);
    assert_eq!(row["state"], "clean");
}

#[test]
fn behind_upstream_reports_behind() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "behind");
    let branch = current_branch(&repo);
    let bare = attach_upstream(&repo, &branch);
    add_upstream_commits(&repo, &bare, &branch, 3);
    register(&config_dir, &repo, "behind");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["ahead"], 0);
    assert_eq!(row["behind"], 3);
    assert_eq!(row["state"], "clean");
}

#[test]
fn ahead_and_behind_simultaneously() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "diverged");
    let branch = current_branch(&repo);
    let bare = attach_upstream(&repo, &branch);
    add_upstream_commits(&repo, &bare, &branch, 2);
    add_local_commits(&repo, 1);
    register(&config_dir, &repo, "diverged");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert_eq!(row["ahead"], 1);
    assert_eq!(row["behind"], 2);
}

#[test]
fn local_only_branch_has_no_upstream() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "local-only");
    register(&config_dir, &repo, "local-only");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert!(row["upstream"].is_null());
    assert_eq!(row["ahead"], 0);
    assert_eq!(row["behind"], 0);
}

// ─── output contract ───────────────────────────────────────────────────────

#[test]
fn empty_registry_json_is_empty_repos_array() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let body = std::str::from_utf8(&out.get_output().stdout)
        .unwrap()
        .trim();
    assert_eq!(body, "{\"repos\":[]}");
}

#[test]
fn error_field_is_present_and_null_on_healthy_rows() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "ok");
    register(&config_dir, &repo, "ok");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let body = std::str::from_utf8(&out.get_output().stdout).unwrap();
    assert!(
        body.contains("\"error\":null"),
        "healthy row carries `\"error\":null` literally, got: {body}"
    );
}

#[test]
fn stdout_is_only_json_when_piped() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "ok");
    register(&config_dir, &repo, "ok");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&out.get_output().stdout).unwrap();
    // Pipes cleanly: stdout is valid JSON exactly.
    let v: serde_json::Value = serde_json::from_str(stdout).expect("stdout is JSON");
    assert!(v["repos"].is_array());
}

#[test]
fn malformed_toml_exits_1() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), "this = = is bad [[[").unwrap();

    repograph_cmd(&config_dir).arg("status").assert().code(1);
}

// ─── --fetch ────────────────────────────────────────────────────────────────

#[test]
fn fetch_updates_behind_count() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "stale");
    let branch = current_branch(&repo);
    let bare = attach_upstream(&repo, &branch);
    // After the initial attach_upstream, local already saw `origin/<branch>`
    // pointed at HEAD. Add upstream commits WITHOUT calling
    // add_upstream_commits' final fetch — we want --fetch to do the fetch.
    push_upstream_only(&bare, &branch, 2);

    register(&config_dir, &repo, "stale");

    // Without --fetch: still 0/0 (we haven't fetched).
    let out_no_fetch = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out_no_fetch.get_output().stdout)[0].clone();
    assert_eq!(row["behind"], 0, "no fetch → stale view");

    // With --fetch: behind = 2.
    let out_fetch = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .arg("--fetch")
        .assert()
        .success();
    let row = parse_status_json(&out_fetch.get_output().stdout)[0].clone();
    assert_eq!(row["behind"], 2, "--fetch refreshes ahead/behind");
}

#[test]
fn fetch_isolated_failure_populates_only_failing_row() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let good = fixture_git_repo(tmp.path(), "good");
    let good_branch = current_branch(&good);
    attach_upstream(&good, &good_branch);

    let bad = fixture_git_repo(tmp.path(), "bad");
    let bad_branch = current_branch(&bad);
    // Configure a bogus upstream URL that will fail to fetch.
    {
        let repo = git2::Repository::open(&bad).unwrap();
        repo.remote("origin", "/nonexistent/path/that/cannot/exist")
            .unwrap();
        let mut cfg = repo.config().unwrap();
        cfg.set_str(&format!("branch.{bad_branch}.remote"), "origin")
            .unwrap();
        cfg.set_str(
            &format!("branch.{bad_branch}.merge"),
            &format!("refs/heads/{bad_branch}"),
        )
        .unwrap();
    }

    register(&config_dir, &good, "good");
    register(&config_dir, &bad, "bad");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .arg("--fetch")
        .assert()
        .success();
    let rows = parse_status_json(&out.get_output().stdout);
    let bad_row = rows.iter().find(|r| r["name"] == "bad").unwrap();
    let good_row = rows.iter().find(|r| r["name"] == "good").unwrap();
    assert!(
        !bad_row["error"].is_null(),
        "failing fetch populates error: {bad_row}"
    );
    assert!(
        good_row["error"].is_null(),
        "healthy repo unaffected: {good_row}"
    );
}

/// `git2` is built with `default-features = false`. The `https` transport is
/// only available when the `https` feature is explicitly enabled — without
/// it, every `https://` upstream fails with `"unsupported URL protocol"`,
/// which is the exact bug we caught when smoke-testing against a real GitHub
/// remote. This test pins that the HTTPS transport is compiled in by
/// targeting a guaranteed-closed port (127.0.0.1:1): the fetch MUST fail
/// (network error), but the error message MUST NOT be the transport-missing
/// signature.
#[test]
fn fetch_supports_https_transport() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "https-target");
    let branch = current_branch(&repo);
    configure_upstream_url(&repo, &branch, "https://127.0.0.1:1/nope.git");
    register(&config_dir, &repo, "https-target");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .arg("--fetch")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    let err = row["error"].as_str().unwrap_or("").to_lowercase();
    assert!(
        !err.is_empty(),
        "closed-port HTTPS fetch must fail with some error: {row}"
    );
    assert!(
        !err.contains("unsupported url protocol"),
        "HTTPS transport must be compiled in (git2 `https` feature), got: {err}"
    );
    assert!(
        !err.contains("no callback set"),
        "credential callback must be wired for HTTPS fetches, got: {err}"
    );
}

/// Same shape as the HTTPS test but for the `ssh` feature. Targets a
/// guaranteed-closed port so the test stays offline and fast and doesn't
/// hang on real SSH auth.
#[test]
fn fetch_supports_ssh_transport() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "ssh-target");
    let branch = current_branch(&repo);
    configure_upstream_url(&repo, &branch, "ssh://git@127.0.0.1:1/nope.git");
    register(&config_dir, &repo, "ssh-target");

    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .arg("--fetch")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    let err = row["error"].as_str().unwrap_or("").to_lowercase();
    assert!(
        !err.is_empty(),
        "closed-port SSH fetch must fail with some error: {row}"
    );
    assert!(
        !err.contains("unsupported url protocol"),
        "SSH transport must be compiled in (git2 `ssh` feature), got: {err}"
    );
    assert!(
        !err.contains("no callback set"),
        "credential callback must be wired for SSH fetches, got: {err}"
    );
}

#[test]
fn no_fetch_flag_does_not_touch_network() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "off");
    let branch = current_branch(&repo);
    // Point upstream at a path that would fail to open if a fetch were attempted.
    {
        let r = git2::Repository::open(&repo).unwrap();
        r.remote("origin", "/nonexistent/never/touched").unwrap();
        let mut cfg = r.config().unwrap();
        cfg.set_str(&format!("branch.{branch}.remote"), "origin")
            .unwrap();
        cfg.set_str(
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{branch}"),
        )
        .unwrap();
    }
    register(&config_dir, &repo, "off");

    // No --fetch flag: command succeeds and produces a clean row regardless of
    // the broken upstream URL.
    let out = repograph_cmd(&config_dir)
        .arg("status")
        .arg("--json")
        .assert()
        .success();
    let row = parse_status_json(&out.get_output().stdout)[0].clone();
    assert!(row["error"].is_null());
}

// ─── Helpers private to this test file ─────────────────────────────────────

/// Wire `repo`'s `branch` to an `origin` remote with the given URL — no push,
/// no fetch. Used to plant a remote that will fail at transport time so we can
/// observe the failure mode.
fn configure_upstream_url(repo_path: &Path, branch: &str, url: &str) {
    let repo = git2::Repository::open(repo_path).expect("open repo for upstream config");
    repo.remote("origin", url).expect("set origin remote");
    let mut cfg = repo.config().expect("local config");
    cfg.set_str(&format!("branch.{branch}.remote"), "origin")
        .expect("branch.remote");
    cfg.set_str(
        &format!("branch.{branch}.merge"),
        &format!("refs/heads/{branch}"),
    )
    .expect("branch.merge");
}

fn current_branch(repo_path: &Path) -> String {
    let repo = git2::Repository::open(repo_path).expect("open repo for branch lookup");
    let head = repo.head().expect("HEAD");
    head.shorthand().expect("HEAD has a short name").to_string()
}

/// Push upstream commits to the bare without fetching them on the local. Used
/// to manufacture a "behind" state that only `--fetch` can resolve.
fn push_upstream_only(bare: &Path, branch: &str, count: usize) {
    let parent = bare.parent().expect("bare has parent");
    let work = parent.join("upstream-fetch-only");
    if work.exists() {
        std::fs::remove_dir_all(&work).expect("clean upstream work");
    }
    let cloned = git2::Repository::clone(&bare.to_string_lossy(), &work)
        .expect("clone bare for upstream commits");
    let sig = git2::Signature::now("Test", "test@example.com").expect("signature");
    for i in 0..count {
        let parent_commit = cloned
            .head()
            .expect("HEAD")
            .peel_to_commit()
            .expect("parent");
        let tree = parent_commit.tree().expect("parent tree");
        cloned
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("upstream-only commit {i}"),
                &tree,
                &[&parent_commit],
            )
            .expect("upstream-only commit");
    }
    {
        let mut origin = cloned.find_remote("origin").expect("clone origin");
        origin
            .push(&[&format!("refs/heads/{branch}:refs/heads/{branch}")], None)
            .expect("push upstream commits back to bare");
    }
    drop(cloned);
    std::fs::remove_dir_all(&work).expect("clean upstream work after push");
}
