//! Acceptance tests for `repograph switch`.
//!
//! Each spec scenario in
//! `openspec/changes/shell-integration/specs/shell-integration/spec.md`
//! is represented by at least one test below.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use tempfile::TempDir;

use crate::common::{fixture_git_repo, repograph_cmd};

fn register(config_dir: &Path, repo: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

#[test]
fn successful_switch_emits_exactly_cd_line() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("api")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&out.get_output().stdout).unwrap();
    let expected = format!("cd {}\n", api.display());
    assert_eq!(
        stdout, expected,
        "stdout is exactly `cd <path>\\n`, got: {stdout:?}"
    );
}

#[test]
fn unknown_repo_exits_3_with_empty_stdout() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("nope")
        .assert()
        .code(3);
    assert!(
        out.get_output().stdout.is_empty(),
        "stdout zero bytes on miss"
    );
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(stderr.contains("nope"), "stderr names the lookup: {stderr}");
}

#[test]
fn near_miss_suggestion_appears_on_stderr() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("app")
        .assert()
        .code(3);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        stderr.contains("did you mean") && stderr.contains("api"),
        "stderr has `did you mean: api`: {stderr}"
    );
}

#[test]
fn no_near_miss_means_no_suggestion() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("zzzz")
        .assert()
        .code(3);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    assert!(
        !stderr.contains("did you mean"),
        "no suggestion when no near-miss: {stderr}"
    );
}

#[test]
fn switch_works_without_agents_section() {
    // No `init` invoked first → config has no `[agents]` section.
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("api")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&out.get_output().stdout).unwrap();
    assert!(stdout.starts_with("cd "), "no NeedsInit raised: {stdout}");
}

#[test]
#[cfg(unix)]
fn path_with_space_is_single_quoted() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let parent = tmp.path().join("has space");
    let repo = fixture_git_repo(&parent, "repo");
    register(&config_dir, &repo, "spacey");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("spacey")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&out.get_output().stdout).unwrap();
    assert_eq!(stdout, format!("cd '{}'\n", repo.display()));
}

#[test]
#[cfg(unix)]
fn path_with_embedded_single_quote_uses_escape_sequence() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let parent = tmp.path().join("mike's");
    let repo = fixture_git_repo(&parent, "repo");
    register(&config_dir, &repo, "quoted");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("quoted")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&out.get_output().stdout).unwrap();
    assert!(
        stdout.contains("'\\''"),
        "embedded `'` escaped as `'\\''`: {stdout:?}"
    );
}

#[test]
fn stdout_only_no_log_leak() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("switch")
        .arg("api")
        .assert()
        .success();
    let stdout = std::str::from_utf8(&out.get_output().stdout).unwrap();
    // Exactly one line, starts with `cd `, ends with `\n`. No banner / log line.
    assert!(stdout.starts_with("cd "));
    assert!(stdout.ends_with('\n'));
    assert_eq!(
        stdout.lines().count(),
        1,
        "exactly one line on stdout, got: {stdout:?}"
    );
}
