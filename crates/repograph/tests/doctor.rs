//! Acceptance tests for `repograph doctor`.
//!
//! Each spec scenario in
//! `openspec/changes/shell-integration/specs/doctor-command/spec.md` is
//! represented by at least one test below.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_files, fixture_git_repo, fixture_git_repo_with_files, repograph_cmd};

fn init_agents(config_dir: &Path, agents: &str) {
    let cwd = config_dir
        .parent()
        .expect("config_dir always lives under a tempdir");
    repograph_cmd(config_dir)
        .current_dir(cwd)
        // Pin HOME so the install (and the later doctor freshness check) resolve
        // skill-artifact paths under the fixture, never the dev's real home.
        .env("HOME", cwd)
        .env("USERPROFILE", cwd)
        .arg("init")
        .arg("--no-prompt")
        .arg("--agents")
        .arg(agents)
        .arg("--scope")
        .arg("project")
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

fn run_doctor_json(config_dir: &Path) -> (serde_json::Value, i32) {
    let cwd = config_dir
        .parent()
        .expect("config_dir always lives under a tempdir");
    let out = repograph_cmd(config_dir)
        // Same HOME/cwd the install used, so the read-only skill freshness check
        // resolves the artifacts the fixture wrote — and never the real home.
        .current_dir(cwd)
        .env("HOME", cwd)
        .env("USERPROFILE", cwd)
        .arg("doctor")
        .arg("--json")
        .assert();
    let code = out.get_output().status.code().unwrap_or(-1);
    let v: serde_json::Value =
        serde_json::from_slice(&out.get_output().stdout).expect("stdout parses as JSON");
    (v, code)
}

fn find_findings<'a>(
    v: &'a serde_json::Value,
    check: &str,
    severity: &str,
) -> Vec<&'a serde_json::Value> {
    v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["check"] == check && f["severity"] == severity)
        .collect()
}

#[test]
fn clean_config_emits_all_ok_and_exit_0() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    std::fs::write(api.join("CLAUDE.md"), "ctx\n").unwrap();
    let ui = fixture_git_repo(tmp.path(), "ui");
    std::fs::write(ui.join("CLAUDE.md"), "ctx\n").unwrap();
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");
    register(&config_dir, &ui, "ui");
    create_workspace(&config_dir, "team");
    add_to_workspace(&config_dir, "team", &["api", "ui"]);
    // Build the search index so the SearchIndex check reports `ok` rather than
    // the "no index built yet" warn — this is a fully healthy setup.
    repograph_cmd(&config_dir).arg("index").assert().success();

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0);
    assert_eq!(v["schema_version"], 1);
    assert!(v["generated_at"].is_string());
    assert_eq!(v["summary"]["error"], 0);
    assert_eq!(v["summary"]["warn"], 0);
    assert!(v["summary"]["ok"].as_u64().unwrap() > 0);
    // The search-index check is present and healthy.
    assert_eq!(find_findings(&v, "SearchIndex", "ok").len(), 1);
    // Both skill artifacts (consumer + setup) are present and current.
    assert_eq!(find_findings(&v, "SkillArtifactFresh", "ok").len(), 2);
}

#[test]
fn missing_skill_artifact_emits_warn_with_init_hint() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    init_agents(&config_dir, "claude-code");
    // Remove the installed setup skill so the freshness check sees it missing.
    let cwd = config_dir.parent().unwrap();
    std::fs::remove_dir_all(cwd.join(".claude/skills/repograph-setup")).unwrap();

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0, "a missing skill artifact is a warn, not an error");
    let warns = find_findings(&v, "SkillArtifactFresh", "warn");
    assert_eq!(warns.len(), 1, "the removed setup skill warns");
    assert!(
        warns[0]["message"]
            .as_str()
            .unwrap()
            .contains("repograph init"),
        "warning must point at `repograph init`"
    );
    assert!(
        warns[0]["target"]
            .as_str()
            .unwrap()
            .contains("repograph-setup"),
        "target names the missing capability"
    );
}

#[test]
fn search_index_not_built_emits_warn() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn a() {}\n")]);
    register(&config_dir, &api, "api");
    // No `repograph index` run.
    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0, "missing index is a warn, not an error");
    let warns = find_findings(&v, "SearchIndex", "warn");
    assert_eq!(warns.len(), 1);
    assert!(
        warns[0]["message"]
            .as_str()
            .unwrap()
            .contains("repograph index")
    );
}

#[test]
fn search_index_stale_after_new_commit_emits_warn_naming_repo() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo_with_files(tmp.path(), "api", &[("a.rs", "fn a() {}\n")]);
    register(&config_dir, &api, "api");
    repograph_cmd(&config_dir).arg("index").assert().success();
    // A new commit moves HEAD past the indexed commit.
    commit_files(&api, "more", &[("b.rs", "fn b() {}\n")]);

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0);
    let warns = find_findings(&v, "SearchIndex", "warn");
    assert_eq!(warns.len(), 1, "stale index warns");
    let names_repo = warns[0]["message"].as_str().unwrap().contains("api")
        || warns[0]["target"].as_str().unwrap().contains("api");
    assert!(names_repo, "stale finding names the repo");
}

#[test]
fn missing_repo_path_emits_repo_path_exists_error_and_exit_1() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");
    // Forcibly delete after registration so the registry now points at a
    // missing path.
    std::fs::remove_dir_all(&api).unwrap();

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 1);
    let errs = find_findings(&v, "RepoPathExists", "error");
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0]["target"], "api");
}

#[test]
fn non_git_path_emits_repo_path_ok_and_repo_is_git_repo_error() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let plain = tmp.path().join("notes");
    std::fs::create_dir_all(&plain).unwrap();
    // Cannot use repograph add (it validates git-repo-ness). Build the config
    // via the registry directly through init + writing config.toml.
    let canonical = repograph_core::path::canonicalize(&plain).unwrap();
    let body = format!(
        "[repo.notes]\npath = \"{}\"\n",
        canonical.display().to_string().replace('\\', "\\\\")
    );
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), body).unwrap();

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 1);
    assert_eq!(find_findings(&v, "RepoPathExists", "ok").len(), 1);
    assert_eq!(find_findings(&v, "RepoIsGitRepo", "error").len(), 1);
}

#[test]
fn dangling_workspace_member_emits_warn_and_exit_0() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");
    create_workspace(&config_dir, "acme");
    add_to_workspace(&config_dir, "acme", &["api"]);
    // Deregister `api` so the workspace member becomes dangling.
    repograph_cmd(&config_dir)
        .arg("remove")
        .arg("api")
        .assert()
        .success();

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0);
    let warns = find_findings(&v, "WorkspaceMembersResolve", "warn");
    assert_eq!(warns.len(), 1);
    assert_eq!(warns[0]["target"], "acme");
}

#[test]
fn missing_agent_doc_emits_warn_and_exit_0() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api"); // no CLAUDE.md
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0);
    let warns = find_findings(&v, "AgentDocPresent", "warn");
    assert_eq!(warns.len(), 1);
    assert_eq!(warns[0]["target"], "api / claude-code");
}

#[test]
fn missing_agents_section_emits_warn_and_no_agent_doc_findings() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api"); // no init → no [agents]

    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 0);
    let warns = find_findings(&v, "AgentsConfigured", "warn");
    assert_eq!(warns.len(), 1);
    // Gate held: no AgentDocPresent findings at all.
    assert!(find_findings(&v, "AgentDocPresent", "ok").is_empty());
    assert!(find_findings(&v, "AgentDocPresent", "warn").is_empty());
}

#[test]
fn missing_config_file_emits_config_present_error_and_exit_1() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    // No file written; config_dir doesn't even exist yet.
    let (v, code) = run_doctor_json(&config_dir);
    assert_eq!(code, 1);
    let errs = find_findings(&v, "ConfigPresent", "error");
    assert_eq!(errs.len(), 1);
}

#[test]
#[cfg(unix)]
fn config_permission_denied_exits_4_with_empty_stdout() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("config.toml");
    std::fs::write(&path, "[repo.api]\npath = \"/tmp/api\"\n").unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&path, perms).unwrap();

    let out = repograph_cmd(&config_dir).arg("doctor").assert().code(4);
    assert!(
        out.get_output().stdout.is_empty(),
        "stdout empty on perm denied"
    );
    // Restore so the tempdir can clean up.
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&path, perms).unwrap();
}

#[test]
fn stdout_only_no_log_leak_in_json_mode() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir)
        .arg("doctor")
        .arg("--json")
        .assert();
    let stdout = out.get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("parses");
    assert_eq!(v["schema_version"], 1);
}

#[test]
fn json_sort_order_severity_desc_check_asc_target_asc() {
    // Manufacture a config that yields one error + multiple warns + multiple oks.
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api"); // no CLAUDE.md → AgentDocPresent warn
    register(&config_dir, &api, "api");
    create_workspace(&config_dir, "acme");
    add_to_workspace(&config_dir, "acme", &["api"]);
    repograph_cmd(&config_dir)
        .arg("remove")
        .arg("api")
        .assert()
        .success(); // dangling member warn
    // Re-register api but at a now-missing path to manufacture an error.
    let gone = tmp.path().join("does-not-exist");
    let body = format!(
        "[repo.api]\npath = \"{}\"\n\n[workspace.acme]\nmembers = [\"api\"]\n",
        gone.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(config_dir.join("config.toml"), body).unwrap();

    let (v, _) = run_doctor_json(&config_dir);
    let checks = v["checks"].as_array().unwrap();
    // First finding must have severity "error".
    assert_eq!(checks[0]["severity"], "error");
    // Severities are non-increasing through the array.
    let order = |s: &str| match s {
        "error" => 0,
        "warn" => 1,
        "ok" => 2,
        _ => 99,
    };
    let severities: Vec<&str> = checks
        .iter()
        .map(|f| f["severity"].as_str().unwrap())
        .collect();
    for w in severities.windows(2) {
        assert!(
            order(w[0]) <= order(w[1]),
            "severity non-decreasing in priority order: {severities:?}"
        );
    }
}

#[test]
fn summary_totals_match_checks_length() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    init_agents(&config_dir, "claude-code");
    register(&config_dir, &api, "api");

    let (v, _) = run_doctor_json(&config_dir);
    let s = &v["summary"];
    let total = s["total"].as_u64().unwrap();
    let ok = s["ok"].as_u64().unwrap();
    let warn = s["warn"].as_u64().unwrap();
    let err = s["error"].as_u64().unwrap();
    assert_eq!(total, ok + warn + err);
    assert_eq!(total, v["checks"].as_array().unwrap().len() as u64);
}

#[test]
fn non_tty_without_json_still_emits_json() {
    // assert_cmd runs without a TTY by default, so the no-flag invocation
    // should produce JSON (per the doctor spec's non-TTY behavior).
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("config");
    let api = fixture_git_repo(tmp.path(), "api");
    register(&config_dir, &api, "api");

    let out = repograph_cmd(&config_dir).arg("doctor").assert();
    let stdout = out.get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("parses as JSON in non-TTY");
    assert_eq!(v["schema_version"], 1);
}
