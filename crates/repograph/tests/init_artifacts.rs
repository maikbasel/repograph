//! Acceptance tests for the per-agent artifact install layer.
//!
//! Drives the `repograph init` binary end-to-end and asserts the right files
//! land at the right paths for each agent in the v1 matrix, that re-runs are
//! idempotent, that `--force` rewrites the file fresh, that pre-existing user
//! content is preserved by default, and that the stdout contract holds.
//!
//! The settings-panel `Update agent selection` flow shares `run_install` with
//! the non-interactive path, so its behavior is exercised end-to-end here via
//! chained `--no-prompt` invocations. The cliclack UX itself is covered by
//! the manual validation script (see `openspec/changes/agent-skills/tasks.md`
//! task 14.5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common::repograph_cmd;

/// One tempdir housing `home/`, `proj/`, and `config/` so a single test can
/// drive the binary with a fully isolated environment: `HOME` for user-scope
/// paths, `current_dir` for project-scope paths, and `REPOGRAPH_CONFIG_DIR`
/// for the config file. Holds the `TempDir` so it survives until the test
/// drops the fixture.
struct InitFixture {
    _tmp: TempDir,
    home: PathBuf,
    proj: PathBuf,
    config: PathBuf,
}

impl InitFixture {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let proj = tmp.path().join("proj");
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        Self {
            _tmp: tmp,
            home,
            proj,
            config,
        }
    }

    /// Build a `repograph` command with `HOME`, `current_dir`, and
    /// `REPOGRAPH_CONFIG_DIR` wired so every install path resolves under the
    /// fixture. `USERPROFILE` is also set for Windows-host parity (the binary
    /// uses `dirs::home_dir()` which prefers `USERPROFILE` on Windows).
    fn cmd(&self) -> Command {
        let mut cmd = repograph_cmd(&self.config);
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj);
        cmd
    }
}

/// Read a UTF-8 file, panicking on failure (acceptable in tests).
fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

// ---- per-scope, per-agent install paths --------------------------------

#[test]
fn claude_code_user_scope_writes_skill_md_under_home() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "claude-code"])
        .args(["--scope", "user"])
        .assert()
        .success();

    let target = f.home.join(".claude/skills/repograph/SKILL.md");
    assert!(target.exists(), "expected {target:?} to exist");
    let body = read(&target);
    assert!(
        body.starts_with("---\nname: repograph\n"),
        "missing YAML frontmatter, got:\n{body}",
    );
    assert!(
        body.contains("description: >-"),
        "missing folded-scalar description in frontmatter, got:\n{body}",
    );
    assert!(
        body.contains("ALWAYS prefer this over manual"),
        "description should steer the agent away from manual find/git, got:\n{body}",
    );
    assert!(
        body.contains("<!-- repograph:begin -->"),
        "missing begin delimiter, got:\n{body}",
    );
    assert!(
        body.contains("<!-- repograph:end -->"),
        "missing end delimiter, got:\n{body}",
    );
    assert!(
        body.contains("repograph context"),
        "managed body should mention `repograph context`, got:\n{body}",
    );
    assert!(
        body.contains("repograph find"),
        "managed body should teach the cross-repo `repograph find`, got:\n{body}",
    );
}

#[test]
fn claude_code_project_scope_writes_skill_md_under_cwd() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "claude-code"])
        .args(["--scope", "project"])
        .assert()
        .success();

    let target = f.proj.join(".claude/skills/repograph/SKILL.md");
    assert!(target.exists(), "expected {target:?} to exist");
    // The user-scope counterpart MUST NOT have been created.
    assert!(
        !f.home.join(".claude/skills/repograph/SKILL.md").exists(),
        "user-scope path should be untouched under --scope project",
    );
}

#[test]
fn agents_md_no_scope_required_writes_to_cwd() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md"])
        .assert()
        .success();

    let target = f.proj.join("AGENTS.md");
    assert!(target.exists(), "expected {target:?} to exist");
    let body = read(&target);
    assert!(
        body.starts_with("<!-- repograph:begin -->"),
        "agents-md begins with the delimiter (no frontmatter), got:\n{body}",
    );
    assert!(body.contains("# repograph"), "managed heading present");
}

#[test]
fn project_only_agent_falls_through_under_user_scope() {
    let f = InitFixture::new();
    let output = f
        .cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md"])
        .args(["--scope", "user"])
        .output()
        .unwrap();
    assert!(output.status.success(), "init should succeed");

    // File still lands at the project path because agents-md is project-only.
    let target = f.proj.join("AGENTS.md");
    assert!(target.exists(), "expected fall-through to project path");
    // The stderr install summary names the resolved path so an operator can
    // see the fall-through happened.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AGENTS.md"),
        "stderr should mention the resolved path; got:\n{stderr}",
    );
    // No artifact at the user-scope location.
    assert!(
        !f.home.join("AGENTS.md").exists(),
        "user-scope AGENTS.md must not exist",
    );
}

#[test]
fn copilot_selection_succeeds_with_no_artifact() {
    let f = InitFixture::new();
    let output = f
        .cmd()
        .args(["init", "--no-prompt", "--agents", "copilot"])
        .output()
        .unwrap();
    assert!(output.status.success(), "copilot is a valid selection");

    // Config gets [agents] selected = ["copilot"], no artifact files.
    let cfg = read(&f.config.join("config.toml"));
    assert!(cfg.contains("copilot"), "config records copilot:\n{cfg}");

    // No artifact files anywhere under home or proj for v1.
    for p in [
        f.home.join(".claude/skills/repograph/SKILL.md"),
        f.proj.join(".claude/skills/repograph/SKILL.md"),
        f.proj.join("AGENTS.md"),
        f.proj.join("CONVENTIONS.md"),
        f.proj.join(".cursor/rules/repograph.mdc"),
        f.proj.join(".windsurfrules"),
        f.home.join(".codeium/windsurf/memories/repograph.md"),
    ] {
        assert!(
            !p.exists(),
            "no artifact should exist for copilot, found {p:?}"
        );
    }
}

#[test]
fn scope_bearing_agent_without_scope_under_no_prompt_exits_2() {
    let f = InitFixture::new();
    let output = f
        .cmd()
        .args(["init", "--no-prompt", "--agents", "claude-code"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing --scope for scope-bearing agent must exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--scope"),
        "stderr names --scope:\n{stderr}"
    );
    assert!(
        stderr.contains("claude-code"),
        "stderr names the offending agent:\n{stderr}",
    );
    // No config written, no artifact written.
    assert!(
        !f.config.join("config.toml").exists(),
        "no config write on usage error",
    );
    assert!(
        !f.home.join(".claude/skills/repograph/SKILL.md").exists(),
        "no artifact write on usage error",
    );
}

// ---- idempotency & user-content preservation ---------------------------

#[test]
fn re_run_is_idempotent_byte_for_byte() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md"])
        .assert()
        .success();
    let target = f.proj.join("AGENTS.md");
    let first = read(&target);

    let output = f
        .cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md"])
        .output()
        .unwrap();
    assert!(output.status.success(), "second init succeeds");

    let second = read(&target);
    assert_eq!(first, second, "file must be byte-stable across re-runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("artifact unchanged"),
        "stderr should announce Unchanged on re-run; got:\n{stderr}",
    );
}

#[test]
fn pre_existing_user_content_is_preserved() {
    let f = InitFixture::new();
    let target = f.proj.join("AGENTS.md");
    std::fs::write(&target, "# My project\n\nCustom prose.\n").unwrap();

    f.cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md"])
        .assert()
        .success();

    let body = read(&target);
    assert!(
        body.starts_with("# My project\n\nCustom prose.\n"),
        "user prose preserved at top, got:\n{body}",
    );
    assert!(
        body.contains("<!-- repograph:begin -->"),
        "managed block appended, got:\n{body}",
    );
    assert!(
        body.contains("<!-- repograph:end -->"),
        "managed block closed, got:\n{body}",
    );
}

#[test]
fn force_overwrites_pre_existing_user_content() {
    let f = InitFixture::new();
    let target = f.proj.join("AGENTS.md");
    std::fs::write(&target, "# My project\n\nCustom prose.\n").unwrap();

    f.cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md", "--force"])
        .assert()
        .success();

    let body = read(&target);
    assert!(
        body.starts_with("<!-- repograph:begin -->"),
        "force replaced the file with the bare delimited block, got:\n{body}",
    );
    assert!(
        !body.contains("Custom prose."),
        "user content removed under --force, got:\n{body}",
    );
}

// ---- multi-agent & stdout contract --------------------------------------

#[test]
fn multi_agent_install_lands_each_at_its_matrix_path() {
    let f = InitFixture::new();
    f.cmd()
        .args([
            "init",
            "--no-prompt",
            "--agents",
            "claude-code,agents-md,cursor",
        ])
        .args(["--scope", "user"])
        .assert()
        .success();

    // claude-code under user-scope.
    assert!(
        f.home.join(".claude/skills/repograph/SKILL.md").exists(),
        "claude-code user-scope path missing",
    );
    // agents-md and cursor fall through to project-scope.
    assert!(f.proj.join("AGENTS.md").exists(), "agents-md path missing");
    assert!(
        f.proj.join(".cursor/rules/repograph.mdc").exists(),
        "cursor path missing",
    );
}

#[test]
fn stdout_is_empty_for_artifact_install() {
    let f = InitFixture::new();
    let output = f
        .cmd()
        .args(["init", "--no-prompt", "--agents", "claude-code"])
        .args(["--scope", "user"])
        .output()
        .unwrap();
    assert!(output.status.success(), "init succeeds");
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty for init (data contract); got: {}",
        String::from_utf8_lossy(&output.stdout),
    );
    // Per-result lines land on stderr (info!).
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("artifact written"),
        "stderr should announce the Written outcome; got:\n{stderr}",
    );
}

#[test]
fn invalid_scope_value_exits_2_with_empty_stdout() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "claude-code"])
        .args(["--scope", "bogus"])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("user").and(predicate::str::contains("project")));
}

#[test]
fn windsurf_user_scope_writes_under_codeium_dir() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "windsurf"])
        .args(["--scope", "user"])
        .assert()
        .success();

    let target = f.home.join(".codeium/windsurf/memories/repograph.md");
    assert!(target.exists(), "windsurf user-scope path missing");
    let body = read(&target);
    assert!(body.contains("<!-- repograph:begin -->"));
}

#[test]
fn windsurf_project_scope_writes_windsurfrules() {
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "windsurf"])
        .args(["--scope", "project"])
        .assert()
        .success();

    let target = f.proj.join(".windsurfrules");
    assert!(target.exists(), "windsurf project-scope path missing");
}

#[test]
fn switching_selection_writes_new_artifact_but_leaves_old_one() {
    // The settings-panel `Update agent selection` arm and `run_non_interactive`
    // both flow through the same `run_install` helper, so chained
    // `--no-prompt` invocations are a faithful end-to-end stand-in. The
    // cliclack UX itself is covered by the manual validation script.
    let f = InitFixture::new();
    f.cmd()
        .args(["init", "--no-prompt", "--agents", "claude-code"])
        .args(["--scope", "project"])
        .assert()
        .success();
    let old = f.proj.join(".claude/skills/repograph/SKILL.md");
    assert!(old.exists(), "claude-code artifact written on first init");

    f.cmd()
        .args(["init", "--no-prompt", "--agents", "agents-md"])
        .assert()
        .success();

    let new = f.proj.join("AGENTS.md");
    assert!(new.exists(), "agents-md artifact written on second init");
    // Documented behavior: previous artifacts are NOT removed when the
    // selection changes; only the currently-selected agents get a write.
    assert!(
        old.exists(),
        "previous claude-code artifact must be left in place",
    );
}
