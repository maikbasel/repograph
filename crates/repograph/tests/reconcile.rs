//! Acceptance tests for post-upgrade reconciliation.
//!
//! The scenario under test is the one that actually happens to users: a binary
//! is replaced by Homebrew / `cargo install` / the shell installer, none of
//! which run repograph code, and the *next* command must notice and bring the
//! host integration up to date on its own.
//!
//! A stale install is simulated by writing an older `setup_version` into the
//! config, which is exactly the state an upgraded binary finds.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::repograph_cmd;

/// A version guaranteed to be older than anything this binary reports.
const ANCIENT: &str = "0.0.1";

/// An isolated host: `HOME` and cwd both inside a tempdir, so every artifact
/// and MCP config write lands in the fixture rather than the real machine.
struct Host {
    _tmp: TempDir,
    config_dir: std::path::PathBuf,
    home: std::path::PathBuf,
}

impl Host {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        Self {
            _tmp: tmp,
            config_dir,
            home,
        }
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut c = repograph_cmd(&self.config_dir);
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.home);
        c
    }

    fn init_cursor(&self) {
        self.cmd()
            .args(["init", "--no-prompt", "--agents", "cursor"])
            .assert()
            .success();
    }

    fn config_text(&self) -> String {
        std::fs::read_to_string(self.config_dir.join("config.toml")).unwrap()
    }

    /// Rewrite the stamp to simulate a config written by an older binary.
    fn set_stale_stamp(&self) {
        let text = self.config_text();
        let text = if text.contains("setup_version") {
            text.lines()
                .map(|l| {
                    if l.trim_start().starts_with("setup_version") {
                        format!("setup_version = \"{ANCIENT}\"")
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            format!("{text}\n[settings]\nsetup_version = \"{ANCIENT}\"\n")
        };
        std::fs::write(self.config_dir.join("config.toml"), text).unwrap();
    }

    fn cursor_config(&self) -> Option<Value> {
        let p = self.home.join(".cursor").join("mcp.json");
        std::fs::read_to_string(p)
            .ok()
            .map(|t| serde_json::from_str(&t).unwrap())
    }

    fn stamped_version(&self) -> Option<String> {
        let text = self.config_text();
        text.lines()
            .find(|l| l.trim_start().starts_with("setup_version"))
            .map(|l| l.split('"').nth(1).unwrap().to_string())
    }
}

fn remove_registration(home: &Path) {
    let p = home.join(".cursor").join("mcp.json");
    if p.exists() {
        std::fs::remove_file(p).unwrap();
    }
}

#[test]
fn init_registers_the_mcp_server_and_stamps_the_version() {
    let host = Host::new();
    host.init_cursor();

    let cfg = host
        .cursor_config()
        .expect("init should register the MCP server");
    assert_eq!(cfg["mcpServers"]["repograph"]["args"][0], "mcp");
    assert_eq!(cfg["mcpServers"]["repograph"]["args"][1], "serve");
    assert_eq!(
        host.stamped_version().as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "a fresh install is stamped so it never reconciles on its first command"
    );
}

#[test]
fn an_upgraded_binary_registers_mcp_without_a_manual_init() {
    let host = Host::new();
    host.init_cursor();

    // Simulate the pre-MCP world: an install set up by an older binary, whose
    // registration therefore does not exist.
    remove_registration(&host.home);
    host.set_stale_stamp();
    assert!(host.cursor_config().is_none());

    // Any command at all — this one reads nothing and changes nothing.
    host.cmd().arg("list").assert().success();

    let cfg = host
        .cursor_config()
        .expect("reconciliation should have registered the MCP server");
    assert_eq!(cfg["mcpServers"]["repograph"]["args"][1], "serve");
    assert_eq!(
        host.stamped_version().as_deref(),
        Some(env!("CARGO_PKG_VERSION")),
        "the stamp advances so this runs once, not every command"
    );
}

#[test]
fn reconciliation_announces_itself_on_stderr_only() {
    let host = Host::new();
    host.init_cursor();
    remove_registration(&host.home);
    host.set_stale_stamp();

    let out = host.cmd().arg("list").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();

    assert!(
        stderr.contains("updating agent integration"),
        "the user is told what happened, got stderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("updating agent integration"),
        "stdout carries only the command's data, got:\n{stdout}"
    );
    // stdout must still be the clean `list` envelope an agent can parse.
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.get("repos").is_some());
}

#[test]
fn reconciliation_runs_once_not_once_per_command() {
    let host = Host::new();
    host.init_cursor();
    remove_registration(&host.home);
    host.set_stale_stamp();

    host.cmd().arg("list").assert().success();
    let second = host.cmd().arg("list").assert().success();
    let stderr = String::from_utf8_lossy(&second.get_output().stderr).into_owned();

    assert!(
        !stderr.contains("updating agent integration"),
        "the stamp should suppress a second reconciliation, got:\n{stderr}"
    );
}

#[test]
fn an_install_without_agents_is_left_entirely_alone() {
    let host = Host::new();
    // A registry with a repo but no `[agents]` section: `init` was never run.
    let repo = crate::common::fixture_git_repo(host.home.as_path(), "solo");
    host.cmd()
        .arg("add")
        .arg(&repo)
        .args(["--name", "solo"])
        .assert()
        .success();

    host.cmd().arg("list").assert().success();

    assert!(
        host.cursor_config().is_none(),
        "no agent selection means no registration"
    );
    assert!(
        host.stamped_version().is_none(),
        "an unconfigured install must not even be stamped"
    );
}

#[test]
fn a_reconciliation_failure_does_not_change_the_command_exit_code() {
    let host = Host::new();
    host.init_cursor();
    remove_registration(&host.home);
    host.set_stale_stamp();

    // Make the registration target unwritable by planting a regular file where
    // the `.cursor` directory needs to be.
    let cursor_dir = host.home.join(".cursor");
    std::fs::remove_dir_all(&cursor_dir).unwrap();
    std::fs::write(&cursor_dir, "not a directory").unwrap();

    // A successful command stays successful.
    host.cmd().arg("list").assert().success();

    // And a command that fails on its own terms keeps *its* exit code, not one
    // borrowed from the reconciliation failure.
    host.cmd()
        .args(["switch", "does-not-exist"])
        .assert()
        .code(3);
}

#[test]
fn a_stale_binary_path_is_repointed_on_upgrade() {
    let host = Host::new();
    host.init_cursor();

    // Simulate an upgrade that moved the binary: the recorded command points
    // somewhere that no longer resolves.
    let cursor_cfg = host.home.join(".cursor").join("mcp.json");
    std::fs::write(
        &cursor_cfg,
        r#"{"mcpServers":{"repograph":{"command":"/nonexistent/repograph","args":["mcp","serve"]}}}"#,
    )
    .unwrap();
    host.set_stale_stamp();

    host.cmd().arg("list").assert().success();

    let cfg = host.cursor_config().unwrap();
    let command = cfg["mcpServers"]["repograph"]["command"].as_str().unwrap();
    assert_ne!(
        command, "/nonexistent/repograph",
        "reconciliation should repoint a command that no longer resolves"
    );
    assert!(Path::new(command).is_file());
}

#[test]
fn doctor_reports_a_missing_registration_and_fix_repairs_it() {
    let host = Host::new();
    host.init_cursor();
    remove_registration(&host.home);

    let out = host.cmd().args(["doctor", "--json"]).assert();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let warns: Vec<&Value> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["check"] == "McpRegistered" && f["severity"] == "warn")
        .collect();
    assert_eq!(warns.len(), 1, "report: {report}");
    assert!(
        warns[0]["message"]
            .as_str()
            .unwrap()
            .contains("repograph init"),
        "the warning names the command that fixes it"
    );

    let _ = host.cmd().args(["doctor", "--fix"]).assert();
    assert!(
        host.cursor_config().is_some(),
        "`--fix` should repair the registration it just reported"
    );
}

#[test]
fn doctor_emits_no_mcp_finding_for_agents_without_a_host() {
    let host = Host::new();
    host.cmd()
        .args(["init", "--no-prompt", "--agents", "aider"])
        .assert()
        .success();

    let out = host.cmd().args(["doctor", "--json"]).assert();
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(
        !report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["check"] == "McpRegistered"),
        "Aider hosts no MCP server, so it should produce no finding: {report}"
    );
}

#[test]
fn a_stale_registration_is_an_error_that_gates_the_exit_code() {
    let host = Host::new();
    host.init_cursor();

    // Point the registration at a binary that no longer exists — the state a
    // user lands in when a manually-placed install is moved or removed.
    let cursor_cfg = host.home.join(".cursor").join("mcp.json");
    std::fs::write(
        &cursor_cfg,
        r#"{"mcpServers":{"repograph":{"command":"/nonexistent/repograph","args":["mcp","serve"]}}}"#,
    )
    .unwrap();

    // `doctor` exits 1 when any finding is an error.
    let out = host.cmd().args(["doctor", "--json"]).assert().code(1);
    let report: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();

    let errors: Vec<&Value> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["check"] == "McpRegistered" && f["severity"] == "error")
        .collect();
    assert_eq!(errors.len(), 1, "report: {report}");
    assert!(
        errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("/nonexistent/repograph"),
        "the finding names the path that does not resolve"
    );

    // The finding is counted, and the envelope shape is unchanged.
    assert!(report["summary"]["error"].as_u64().unwrap() >= 1);
    assert_eq!(report["schema_version"], 1);
    for key in ["check", "severity", "target", "message"] {
        assert!(
            errors[0].get(key).is_some(),
            "finding keeps the existing `{key}` field"
        );
    }
}

#[test]
fn project_scope_init_never_writes_into_the_home_directory() {
    // Regression: `init` passes `--scope` to artifact installation, and
    // registration must honour the same scope. An earlier implementation
    // defaulted to user scope regardless, so a `--scope project` install wrote
    // into the real home directory — including when the test suite ran.
    let host = Host::new();
    let project = host.home.parent().unwrap().join("project");
    std::fs::create_dir_all(&project).unwrap();

    repograph_cmd(&host.config_dir)
        .env("HOME", &host.home)
        .env("USERPROFILE", &host.home)
        .current_dir(&project)
        .args([
            "init",
            "--no-prompt",
            "--agents",
            "cursor",
            "--scope",
            "project",
        ])
        .assert()
        .success();

    assert!(
        project.join(".cursor").join("mcp.json").is_file(),
        "project scope registers under the project"
    );
    assert!(
        !host.home.join(".cursor").exists(),
        "project scope must not touch the home directory"
    );
}
