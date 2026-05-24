//! Acceptance tests for `repograph context`.
//!
//! Mirrors the structure of `tests/status.rs`. Each spec scenario from
//! `openspec/changes/context-command/specs/context-command/spec.md` is
//! represented by at least one acceptance test here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use tempfile::TempDir;

use crate::common::{fixture_git_repo, repograph_cmd};

/// Run `repograph init --no-prompt --agents <list>` against `config_dir` so
/// subsequent `context` invocations have `[agents]` configured without
/// touching the cliclack flow.
fn init_agents(config_dir: &Path, agents: &str) {
    repograph_cmd(config_dir)
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg(agents)
        .assert()
        .success();
}

fn register(config_dir: &Path, repo: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("add")
        .arg(repo)
        .arg("--name")
        .arg(name)
        .assert()
        .success();
}

fn create_workspace(config_dir: &Path, name: &str) {
    repograph_cmd(config_dir)
        .arg("workspace")
        .arg("create")
        .arg(name)
        .assert()
        .success();
}

fn add_to_workspace(config_dir: &Path, workspace: &str, repos: &[&str]) {
    let mut cmd = repograph_cmd(config_dir);
    cmd.arg("workspace").arg("add").arg(workspace);
    for r in repos {
        cmd.arg(r);
    }
    cmd.assert().success();
}

fn parse_context_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).expect("stdout is valid JSON")
}

// ─── default scope, JSON happy path ─────────────────────────────────────────

#[test]
fn default_scope_json_includes_every_registered_repo() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    std::fs::write(api.join("CLAUDE.md"), "api ctx\n").unwrap();
    let ui = fixture_git_repo(tmp.path(), "ui");
    std::fs::write(ui.join("CLAUDE.md"), "ui ctx\n").unwrap();

    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");
    register(&config_dir, &ui, "ui");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);

    assert_eq!(v["schema_version"], 1);
    assert!(v["generated_at"].is_string());
    assert_eq!(v["agents"][0], "claude-code");
    assert_eq!(v["scope"]["kind"], "all");
    let repos = v["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2);
    // Sorted ascending.
    assert_eq!(repos[0]["name"], "api");
    assert_eq!(repos[1]["name"], "ui");
    assert_eq!(repos[0]["agent_docs"][0]["files"][0]["path"], "CLAUDE.md");
    assert_eq!(repos[0]["agent_docs"][0]["files"][0]["content"], "api ctx\n");
    assert_eq!(repos[1]["agent_docs"][0]["files"][0]["content"], "ui ctx\n");
    assert!(v["warnings"].is_array() && v["warnings"].as_array().unwrap().is_empty());
}

// ─── workspace scope ────────────────────────────────────────────────────────

#[test]
fn workspace_scope_filters_to_members_only() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    let lib = fixture_git_repo(tmp.path(), "lib");
    let ui = fixture_git_repo(tmp.path(), "ui");
    for p in [&api, &lib, &ui] {
        std::fs::write(p.join("CLAUDE.md"), "x\n").unwrap();
    }

    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");
    register(&config_dir, &lib, "lib");
    register(&config_dir, &ui, "ui");
    create_workspace(&config_dir, "backend");
    add_to_workspace(&config_dir, "backend", &["api", "lib"]);

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--workspace")
        .arg("backend")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);

    assert_eq!(v["scope"]["kind"], "workspace");
    assert_eq!(v["scope"]["name"], "backend");
    let repos = v["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2);
    let names: Vec<&str> = repos
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["api", "lib"]);
}

// ─── positional scope ───────────────────────────────────────────────────────

#[test]
fn positional_scope_picks_named_repos_in_user_order_in_scope_echo() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    let lib = fixture_git_repo(tmp.path(), "lib");
    let ui = fixture_git_repo(tmp.path(), "ui");
    for p in [&api, &lib, &ui] {
        std::fs::write(p.join("CLAUDE.md"), "x\n").unwrap();
    }

    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");
    register(&config_dir, &lib, "lib");
    register(&config_dir, &ui, "ui");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("ui")
        .arg("api")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);

    assert_eq!(v["scope"]["kind"], "repos");
    let echoed: Vec<&str> = v["scope"]["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    assert_eq!(echoed, vec!["ui", "api"], "scope echoes user order");

    let names: Vec<&str> = v["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    // repos[] is always sorted.
    assert_eq!(names, vec!["api", "ui"]);
}

// ─── mutual exclusion ───────────────────────────────────────────────────────

#[test]
fn workspace_and_positional_are_mutually_exclusive() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    init_agents(&config_dir, "claude-code");

    repograph_cmd(&config_dir)
        .arg("context")
        .arg("--workspace")
        .arg("any")
        .arg("api")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("cannot be used with"));
}

// ─── unknown name errors ────────────────────────────────────────────────────

#[test]
fn unknown_workspace_exits_3() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    init_agents(&config_dir, "claude-code");

    repograph_cmd(&config_dir)
        .arg("context")
        .arg("--workspace")
        .arg("ghost")
        .arg("--json")
        .assert()
        .failure()
        .code(3)
        .stderr(predicates::str::contains("ghost"));
}

#[test]
fn unknown_positional_repo_exits_3_with_no_payload() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("api")
        .arg("bogus")
        .arg("--json")
        .assert()
        .failure()
        .code(3)
        .stderr(predicates::str::contains("bogus"));
    assert!(
        out.get_output().stdout.is_empty(),
        "no partial payload on unknown-name error, got: {:?}",
        String::from_utf8_lossy(&out.get_output().stdout)
    );
}

// ─── inline-warnings semantics (no aborts) ──────────────────────────────────

#[test]
fn missing_repo_path_yields_placeholder_entry_exit_zero() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let ghost = fixture_git_repo(tmp.path(), "ghost");
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &ghost, "ghost");
    std::fs::remove_dir_all(&ghost).unwrap();

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);
    let entry = &v["repos"][0];
    assert_eq!(entry["name"], "ghost");
    assert!(entry["branch"].is_null());
    assert!(entry["agent_docs"].as_array().unwrap().is_empty());
    let warnings = entry["warnings"].as_array().unwrap();
    assert!(!warnings.is_empty(), "missing path produces warning");
}

#[test]
fn non_utf8_file_skipped_with_warning() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "binary");
    std::fs::write(repo.join(".cursorrules"), [0xFF, 0xFE]).unwrap();
    init_agents(&config_dir, "cursor");
    register(&config_dir, &repo, "binary");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);
    let docs = v["repos"][0]["agent_docs"].as_array().unwrap();
    // Cursor agent block present but with no .cursorrules in files (UTF-8 rejected).
    let cursor_doc = docs.iter().find(|d| d["agent"] == "cursor").unwrap();
    let files = cursor_doc["files"].as_array().unwrap();
    assert!(
        files.iter().all(|f| f["path"] != ".cursorrules"),
        "non-UTF-8 file omitted from files: {files:?}"
    );
    let warnings = v["repos"][0]["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains(".cursorrules")
                && w.as_str().unwrap().contains("UTF-8")),
        "warning names file and reason: {warnings:?}"
    );
}

#[test]
fn glob_expansion_lists_files_sorted_alphabetically() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "r");
    std::fs::create_dir_all(repo.join(".cursor/rules")).unwrap();
    std::fs::write(repo.join(".cursor/rules/b.md"), "b").unwrap();
    std::fs::write(repo.join(".cursor/rules/a.md"), "a").unwrap();
    init_agents(&config_dir, "cursor");
    register(&config_dir, &repo, "r");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);
    let files = v["repos"][0]["agent_docs"][0]["files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        vec![".cursor/rules/a.md", ".cursor/rules/b.md"],
        "files sorted by relative path"
    );
}

#[test]
fn empty_agents_selection_yields_empty_agent_docs() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "r");
    std::fs::write(repo.join("CLAUDE.md"), "x").unwrap();
    register(&config_dir, &repo, "r");
    // Write [agents] section with empty selection directly so we don't have
    // to drive cliclack — mirror how init does it but with no agents.
    let toml = format!(
        "[agents]\nselected = []\n\n[repo.r]\npath = \"{}\"\n",
        repo.display()
    );
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), toml).unwrap();

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);
    assert!(v["agents"].as_array().unwrap().is_empty());
    assert!(
        v["repos"][0]["agent_docs"].as_array().unwrap().is_empty(),
        "no selected agents yields empty agent_docs"
    );
}

// ─── ensure_agents gating ───────────────────────────────────────────────────

#[test]
fn non_tty_without_agents_exits_2_naming_init() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    // Register a repo but DO NOT call init.
    let repo = fixture_git_repo(tmp.path(), "r");
    register(&config_dir, &repo, "r");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("repograph init"));
    assert!(
        out.get_output().stdout.is_empty(),
        "no payload when gating fails, got: {:?}",
        String::from_utf8_lossy(&out.get_output().stdout)
    );
}

// ─── output contract: stdout is clean ───────────────────────────────────────

#[test]
fn stdout_is_pure_json_no_diagnostics_bleed() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "r");
    std::fs::write(repo.join("CLAUDE.md"), "ctx").unwrap();
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &repo, "r");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    // Strict: stdout must parse as JSON with no leading or trailing noise.
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout is exactly JSON");
    assert_eq!(v["schema_version"], 1);
}

// ─── non-TTY default is JSON (no flag needed) ───────────────────────────────

#[test]
fn non_tty_without_json_flag_emits_json() {
    // assert_cmd runs with stdout redirected to a pipe (non-TTY), so the
    // mode detector picks Json without --json.
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "r");
    std::fs::write(repo.join("CLAUDE.md"), "ctx").unwrap();
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &repo, "r");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout)
        .expect("non-TTY default is JSON");
    assert_eq!(v["schema_version"], 1);
}

// ─── default scope echo ─────────────────────────────────────────────────────

#[test]
fn default_scope_echoes_kind_all_only() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "r");
    std::fs::write(repo.join("CLAUDE.md"), "x").unwrap();
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &repo, "r");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);
    assert_eq!(v["scope"]["kind"], "all");
    assert!(v["scope"].get("name").is_none(), "no name field for All");
    assert!(v["scope"].get("repos").is_none(), "no repos field for All");
}

// ─── large file body is verbatim, not truncated ─────────────────────────────

#[test]
fn large_claude_md_is_verbatim_in_payload() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let repo = fixture_git_repo(tmp.path(), "r");
    // 50 KB body with predictable content.
    let body: String = "0123456789".repeat(5_120);
    std::fs::write(repo.join("CLAUDE.md"), &body).unwrap();
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &repo, "r");

    let out = repograph_cmd(&config_dir)
        .arg("context")
        .arg("--json")
        .assert()
        .success();
    let v = parse_context_json(&out.get_output().stdout);
    let file = &v["repos"][0]["agent_docs"][0]["files"][0];
    assert_eq!(file["bytes"].as_u64().unwrap(), body.len() as u64);
    assert_eq!(file["content"].as_str().unwrap(), body);
}
