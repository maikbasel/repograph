//! Acceptance tests for `repograph init`.
//!
//! Covers the non-interactive variant (`--no-prompt --agents <list>`) and the
//! non-TTY guard. The interactive cliclack flow is covered by the manual
//! validation script documented in `openspec/changes/init-command/design.md`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::{fixture_git_repo, repograph_cmd};

#[test]
fn init_no_prompt_happy_path_writes_agents_section() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .current_dir(tmp.path())
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("claude-code,cursor")
        .arg("--scope")
        .arg("project")
        .assert()
        .success();

    let toml_path = config_dir.join("config.toml");
    let body = std::fs::read_to_string(&toml_path).unwrap();
    assert!(
        body.contains("[agents]"),
        "config should contain [agents] section, got:\n{body}"
    );
    assert!(
        body.contains("claude-code"),
        "selection should include claude-code, got:\n{body}"
    );
    assert!(
        body.contains("cursor"),
        "selection should include cursor, got:\n{body}"
    );
}

#[test]
fn init_no_prompt_preserves_selection_order() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .current_dir(tmp.path())
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("cursor,claude-code")
        .arg("--scope")
        .arg("project")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    let cursor_pos = body.find("cursor").expect("cursor present");
    let claude_pos = body.find("claude-code").expect("claude-code present");
    assert!(
        cursor_pos < claude_pos,
        "selection order should be preserved (cursor before claude-code), got:\n{body}"
    );
}

#[test]
fn init_no_prompt_overwrite_preserves_repos_and_workspaces() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo_path = fixture_git_repo(tmp.path(), "preserve-me");

    // Pre-populate config with a repo and a workspace.
    repograph_cmd(&config_dir)
        .arg("add")
        .arg(&repo_path)
        .arg("--name")
        .arg("preserve-me")
        .assert()
        .success();
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
        .arg("preserve-me")
        .assert()
        .success();

    // Now run init in non-interactive mode. `agents-md` is project-only, so
    // `--scope` is not required by the validation; pinning to `project`
    // anyway so the test is robust to future scope-defaulting changes.
    repograph_cmd(&config_dir)
        .current_dir(tmp.path())
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("agents-md")
        .arg("--scope")
        .arg("project")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(
        body.contains("[repo.preserve-me]"),
        "repo entry preserved, got:\n{body}"
    );
    assert!(
        body.contains("[workspace.acme]"),
        "workspace entry preserved, got:\n{body}"
    );
    assert!(
        body.contains("[agents]") && body.contains("agents-md"),
        "agents section overwritten with new selection, got:\n{body}"
    );
}

#[test]
fn init_no_prompt_overwrite_replaces_previous_agents() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .current_dir(tmp.path())
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("claude-code")
        .arg("--scope")
        .arg("project")
        .assert()
        .success();

    repograph_cmd(&config_dir)
        .current_dir(tmp.path())
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("cursor")
        .arg("--scope")
        .arg("project")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(body.contains("cursor"), "new selection present:\n{body}");
    assert!(
        !body.contains("claude-code"),
        "old selection removed:\n{body}"
    );
}

#[test]
fn init_no_prompt_without_agents_exits_2() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("init")
        .arg("--no-prompt")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--agents"));

    assert!(
        !config_dir.join("config.toml").exists(),
        "no config should be written on usage error"
    );
}

#[test]
fn init_no_prompt_unknown_agent_exits_2() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("claude-code,bogus")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("bogus"));

    assert!(
        !config_dir.join("config.toml").exists(),
        "no config should be written on usage error"
    );
}

#[test]
fn init_non_tty_without_flags_exits_2() {
    // `assert_cmd` does not attach a TTY to the child's stdout, so a bare
    // `repograph init` invocation in tests is the non-TTY-without-flags case.
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("init")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("repograph init"));

    assert!(
        !config_dir.join("config.toml").exists(),
        "no config write on non-TTY init without flags"
    );
}

#[test]
fn init_no_prompt_emits_nothing_to_stdout() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    let output = repograph_cmd(&config_dir)
        .current_dir(tmp.path())
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("claude-code")
        .arg("--scope")
        .arg("project")
        .output()
        .unwrap();

    assert!(output.status.success(), "init --no-prompt should succeed");
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty for init (data contract), got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn init_no_prompt_empty_agents_is_valid_configured_state() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");

    repograph_cmd(&config_dir)
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg("")
        .assert()
        .success();

    let body = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(
        body.contains("[agents]"),
        "empty selection still writes [agents] section, got:\n{body}"
    );
}
