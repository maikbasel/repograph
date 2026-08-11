//! Registering `repograph mcp serve` with the agents that host MCP.
//!
//! This is the sibling of [`crate::agent_artifact`]: that module writes the
//! instruction files an agent reads, this one writes the config entry that puts
//! repograph's tools in the agent's tool list. Both are keyed off the same
//! [`AgentId`] registry and both confine themselves to regions they own.
//!
//! # Why the mapping is closed
//!
//! Like `file_patterns()`, the agent → config-file mapping is a fixed table
//! rather than user-extensible. The contract is between repograph and the agent
//! ecosystem; a new agent is a one-arm edit here plus one in `agents.rs`.
//!
//! # Why every agent is written directly, including Claude Code
//!
//! Deferring to `claude mcp add` looks like the polite convention, and it was
//! the original plan. It fails on the property this codebase depends on:
//! `home` and `cwd` are injected everywhere so behaviour is testable against a
//! `tempdir`. A vendor CLI ignores those arguments and resolves its own paths,
//! which means `register` and [`status`] can disagree about which file they are
//! talking about — and a test suite that shells out mutates real user state on
//! whatever machine it runs on.
//!
//! So all four MCP-hosting agents are written directly. Merges preserve every
//! key repograph does not own, which is what made deferring to the vendor
//! attractive in the first place.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::agent_artifact::Scope;
use crate::agents::AgentId;
use crate::error::RepographError;

/// The key repograph registers itself under in every client's server map.
pub const SERVER_KEY: &str = "repograph";

/// Arguments the registered command is invoked with.
pub const SERVE_ARGS: [&str; 2] = ["mcp", "serve"];

/// Top-level key holding the server map in every supported client's config.
const SERVERS_KEY: &str = "mcpServers";

/// VS Code / Copilot names the same map `servers` rather than `mcpServers`.
const VSCODE_SERVERS_KEY: &str = "servers";

/// Where a given agent stores its MCP server map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// Agent hosts MCP via a JSON config file repograph merges into.
    ConfigFile {
        /// Absolute path to the config file.
        path: PathBuf,
        /// The key the server map lives under.
        servers_key: &'static str,
    },
    /// Agent has no MCP host; artifact-only integration.
    None,
}

/// Outcome of registering one agent. Mirrors [`crate::ArtifactResult`]'s shape
/// so the two can be reported side by side.
#[derive(Debug, Clone)]
pub enum RegistrationResult {
    /// Entry was created or corrected.
    Written { agent: AgentId, target: String },
    /// Entry already pointed at this executable; no write occurred.
    Unchanged { agent: AgentId, target: String },
    /// Agent hosts no MCP server; nothing was attempted.
    Skipped {
        agent: AgentId,
        reason: &'static str,
    },
    /// Registration failed. Reported on stderr; never aborts the surrounding run.
    Failed { agent: AgentId, error: String },
}

impl RegistrationResult {
    /// The agent this result describes.
    #[must_use]
    pub const fn agent(&self) -> AgentId {
        match self {
            Self::Written { agent, .. }
            | Self::Unchanged { agent, .. }
            | Self::Skipped { agent, .. }
            | Self::Failed { agent, .. } => *agent,
        }
    }
}

/// Reason recorded when an agent has no MCP host.
pub const REASON_NO_MCP_HOST: &str = "agent hosts no MCP server";

/// Resolve where an agent's MCP registration lives.
///
/// `home` and `cwd` are injected rather than read from the environment so tests
/// can drive this against a `tempdir`, matching `agent_artifact::resolve_path`.
#[must_use]
pub fn host_for(agent: AgentId, scope: Scope, home: &Path, cwd: &Path) -> Host {
    match agent {
        // User scope is Claude Code's own `~/.claude.json`; project scope is the
        // dedicated `.mcp.json` it reads from a repo root. Merging into
        // `.claude.json` preserves the session and project state it also keeps
        // there — only the `mcpServers.repograph` key is touched.
        AgentId::ClaudeCode => Host::ConfigFile {
            path: match scope {
                Scope::User => home.join(".claude.json"),
                Scope::Project => cwd.join(".mcp.json"),
            },
            servers_key: SERVERS_KEY,
        },
        AgentId::Cursor => Host::ConfigFile {
            path: match scope {
                Scope::User => home.join(".cursor").join("mcp.json"),
                Scope::Project => cwd.join(".cursor").join("mcp.json"),
            },
            servers_key: SERVERS_KEY,
        },
        AgentId::Windsurf => Host::ConfigFile {
            path: home
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
            servers_key: SERVERS_KEY,
        },
        AgentId::Copilot => Host::ConfigFile {
            path: cwd.join(".vscode").join("mcp.json"),
            servers_key: VSCODE_SERVERS_KEY,
        },
        // Aider reads CONVENTIONS.md and AGENTS.md is a file convention, not a
        // runtime; neither spawns MCP servers.
        AgentId::Aider | AgentId::AgentsMd => Host::None,
    }
}

/// Does this agent host an MCP server?
///
/// Matched directly rather than derived from [`host_for`] so it can be `const`
/// — the artifact writer needs it at compile time to pick a body variant. The
/// exhaustiveness test below keeps the two in step.
#[must_use]
pub const fn hosts_mcp(agent: AgentId) -> bool {
    match agent {
        AgentId::ClaudeCode | AgentId::Cursor | AgentId::Windsurf | AgentId::Copilot => true,
        AgentId::Aider | AgentId::AgentsMd => false,
    }
}

/// The server entry repograph registers: the resolved executable plus args.
///
/// The absolute path of the running executable is used rather than the bare
/// name, because a client spawning the server does not necessarily inherit the
/// `PATH` the user's shell had.
fn server_entry(exe: &Path) -> Value {
    serde_json::json!({
        "command": exe.to_string_lossy(),
        "args": SERVE_ARGS,
    })
}

/// Register the MCP server for one agent.
///
/// Idempotent: an entry already pointing at `exe` is left alone, an entry
/// pointing elsewhere is corrected, and entries for other servers are preserved.
#[must_use]
pub fn register(
    agent: AgentId,
    scope: Scope,
    home: &Path,
    cwd: &Path,
    exe: &Path,
) -> RegistrationResult {
    match host_for(agent, scope, home, cwd) {
        Host::None => RegistrationResult::Skipped {
            agent,
            reason: REASON_NO_MCP_HOST,
        },
        Host::ConfigFile { path, servers_key } => merge_into_config(agent, &path, servers_key, exe),
    }
}

/// Merge the repograph entry into a client's JSON config, preserving everything
/// else in the file.
fn merge_into_config(
    agent: AgentId,
    path: &Path,
    servers_key: &str,
    exe: &Path,
) -> RegistrationResult {
    let target = path.display().to_string();

    let mut root = match read_json_object(path) {
        Ok(v) => v,
        Err(e) => {
            return RegistrationResult::Failed {
                agent,
                error: format!("{target}: {e}"),
            };
        }
    };

    let desired = server_entry(exe);
    let servers = root
        .entry(servers_key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(servers) = servers.as_object_mut() else {
        return RegistrationResult::Failed {
            agent,
            error: format!("{target}: `{servers_key}` is not a JSON object"),
        };
    };

    if servers.get(SERVER_KEY) == Some(&desired) {
        return RegistrationResult::Unchanged { agent, target };
    }
    servers.insert(SERVER_KEY.to_string(), desired);

    match write_json_object(path, &root) {
        Ok(()) => RegistrationResult::Written { agent, target },
        Err(e) => RegistrationResult::Failed {
            agent,
            error: format!("{target}: {e}"),
        },
    }
}

/// Read a JSON object from `path`, treating a missing file as an empty object.
///
/// A file that exists but holds something other than an object is an error
/// rather than something to overwrite — it belongs to the user's agent, not to
/// repograph.
fn read_json_object(path: &Path) -> Result<Map<String, Value>, RepographError> {
    match fs_err::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(RepographError::Io(e)),
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => {
            let value: Value = serde_json::from_str(&text)
                .map_err(|e| RepographError::Io(std::io::Error::other(e)))?;
            value.as_object().cloned().ok_or_else(|| {
                RepographError::Io(std::io::Error::other("config root is not a JSON object"))
            })
        }
    }
}

fn write_json_object(path: &Path, root: &Map<String, Value>) -> Result<(), RepographError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs_err::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(root)
        .map_err(|e| RepographError::Io(std::io::Error::other(e)))?;
    text.push('\n');
    fs_err::write(path, text)?;
    Ok(())
}

/// Health of one agent's registration, for `doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationStatus {
    /// Registered, and the command it names resolves to an existing file.
    Ok { target: String },
    /// No registration found for this agent.
    Missing { target: String },
    /// Registered, but the command does not resolve.
    Stale { target: String, command: String },
    /// Agent hosts no MCP server.
    NotApplicable,
}

/// Inspect an agent's registration without modifying anything.
///
/// Both scopes are probed and the first *registered* one wins, mirroring how
/// `doctor`'s artifact freshness check searches user scope then project scope.
/// The scope chosen at init time is not recorded in config, so probing is what
/// makes the check work regardless of how the user set things up.
///
/// The vendor-CLI case is inspected through its fallback file rather than by
/// invoking the CLI: `doctor` is read-only, and shelling out to a client to ask
/// a question is both slow and a side-effect risk.
#[must_use]
pub fn status(agent: AgentId, home: &Path, cwd: &Path) -> RegistrationStatus {
    let mut first_missing = None;
    for scope in [Scope::User, Scope::Project] {
        match status_at(agent, scope, home, cwd) {
            RegistrationStatus::NotApplicable => return RegistrationStatus::NotApplicable,
            RegistrationStatus::Missing { target } => {
                first_missing.get_or_insert(RegistrationStatus::Missing { target });
            }
            // A registration that exists — healthy or stale — is the answer.
            found => return found,
        }
    }
    first_missing.unwrap_or(RegistrationStatus::NotApplicable)
}

/// Inspect the registration at one specific scope.
#[must_use]
pub fn status_at(agent: AgentId, scope: Scope, home: &Path, cwd: &Path) -> RegistrationStatus {
    let (path, servers_key) = match host_for(agent, scope, home, cwd) {
        Host::None => return RegistrationStatus::NotApplicable,
        Host::ConfigFile { path, servers_key } => (path, servers_key),
    };
    let target = path.display().to_string();

    let Ok(root) = read_json_object(&path) else {
        return RegistrationStatus::Missing { target };
    };
    let entry = root
        .get(servers_key)
        .and_then(Value::as_object)
        .and_then(|m| m.get(SERVER_KEY));

    let Some(entry) = entry else {
        return RegistrationStatus::Missing { target };
    };
    let command = entry
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if command.is_empty() || !Path::new(&command).is_file() {
        RegistrationStatus::Stale { target, command }
    } else {
        RegistrationStatus::Ok { target }
    }
}

/// The scope an agent is already registered at, if any.
///
/// Reconciliation uses this to re-register where the user's existing entry
/// lives rather than silently creating a second entry at a different scope.
#[must_use]
pub fn registered_scope(agent: AgentId, home: &Path, cwd: &Path) -> Option<Scope> {
    [Scope::User, Scope::Project].into_iter().find(|&scope| {
        matches!(
            status_at(agent, scope, home, cwd),
            RegistrationStatus::Ok { .. } | RegistrationStatus::Stale { .. }
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::TempDir;

    fn exe_fixture(dir: &Path) -> PathBuf {
        let p = dir.join("repograph-bin");
        fs_err::write(&p, b"#!/bin/sh\n").unwrap();
        p
    }

    #[test]
    fn non_mcp_agents_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        for agent in [AgentId::Aider, AgentId::AgentsMd] {
            let r = register(agent, Scope::User, tmp.path(), tmp.path(), &exe);
            assert!(
                matches!(r, RegistrationResult::Skipped { .. }),
                "{agent:?} should be skipped, got {r:?}"
            );
            assert!(!hosts_mcp(agent));
        }
    }

    #[test]
    fn cursor_user_scope_writes_expected_entry() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let r = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(matches!(r, RegistrationResult::Written { .. }), "{r:?}");

        let cfg = tmp.path().join(".cursor").join("mcp.json");
        let v: Value = serde_json::from_str(&fs_err::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["repograph"]["command"],
            exe.to_str().unwrap()
        );
        assert_eq!(v["mcpServers"]["repograph"]["args"][0], "mcp");
        assert_eq!(v["mcpServers"]["repograph"]["args"][1], "serve");
    }

    #[test]
    fn copilot_uses_the_servers_key_not_mcp_servers() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let _ = register(
            AgentId::Copilot,
            Scope::Project,
            tmp.path(),
            tmp.path(),
            &exe,
        );
        let cfg = tmp.path().join(".vscode").join("mcp.json");
        let v: Value = serde_json::from_str(&fs_err::read_to_string(&cfg).unwrap()).unwrap();
        assert!(
            v.get("servers").is_some(),
            "VS Code names the map `servers`"
        );
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn unrelated_servers_survive_registration() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let cfg = tmp.path().join(".cursor").join("mcp.json");
        fs_err::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs_err::write(
            &cfg,
            r#"{"mcpServers":{"other":{"command":"othercmd","args":["x"]}},"unrelatedTopLevel":42}"#,
        )
        .unwrap();

        let _ = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);

        let v: Value = serde_json::from_str(&fs_err::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "othercmd");
        assert_eq!(v["unrelatedTopLevel"], 42);
        assert_eq!(
            v["mcpServers"]["repograph"]["command"],
            exe.to_str().unwrap()
        );
    }

    #[test]
    fn re_registering_is_unchanged_and_does_not_duplicate() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let first = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(matches!(first, RegistrationResult::Written { .. }));
        let second = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(
            matches!(second, RegistrationResult::Unchanged { .. }),
            "second run should be a no-op, got {second:?}"
        );

        let cfg = tmp.path().join(".cursor").join("mcp.json");
        let v: Value = serde_json::from_str(&fs_err::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn a_stale_binary_path_is_corrected() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let cfg = tmp.path().join(".cursor").join("mcp.json");
        fs_err::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs_err::write(
            &cfg,
            r#"{"mcpServers":{"repograph":{"command":"/gone/repograph","args":["mcp","serve"]}}}"#,
        )
        .unwrap();

        let r = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(matches!(r, RegistrationResult::Written { .. }), "{r:?}");
        let v: Value = serde_json::from_str(&fs_err::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["repograph"]["command"],
            exe.to_str().unwrap()
        );
    }

    #[test]
    fn status_reports_ok_missing_and_stale() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());

        assert!(matches!(
            status(AgentId::Cursor, tmp.path(), tmp.path()),
            RegistrationStatus::Missing { .. }
        ));

        let _ = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(matches!(
            status(AgentId::Cursor, tmp.path(), tmp.path()),
            RegistrationStatus::Ok { .. }
        ));

        fs_err::remove_file(&exe).unwrap();
        assert!(matches!(
            status(AgentId::Cursor, tmp.path(), tmp.path()),
            RegistrationStatus::Stale { .. }
        ));
    }

    #[test]
    fn status_is_not_applicable_for_non_mcp_agents() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            status(AgentId::Aider, tmp.path(), tmp.path()),
            RegistrationStatus::NotApplicable
        );
    }

    #[test]
    fn a_non_object_config_root_is_a_failure_not_an_overwrite() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let cfg = tmp.path().join(".cursor").join("mcp.json");
        fs_err::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs_err::write(&cfg, "[1, 2, 3]").unwrap();

        let r = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(matches!(r, RegistrationResult::Failed { .. }), "{r:?}");
        assert_eq!(
            fs_err::read_to_string(&cfg).unwrap(),
            "[1, 2, 3]",
            "a config repograph cannot understand must be left untouched"
        );
    }

    #[test]
    fn an_empty_file_is_treated_as_an_empty_object() {
        let tmp = TempDir::new().unwrap();
        let exe = exe_fixture(tmp.path());
        let cfg = tmp.path().join(".cursor").join("mcp.json");
        fs_err::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs_err::write(&cfg, "   \n").unwrap();

        let r = register(AgentId::Cursor, Scope::User, tmp.path(), tmp.path(), &exe);
        assert!(matches!(r, RegistrationResult::Written { .. }), "{r:?}");
    }

    #[test]
    fn hosts_mcp_agrees_with_host_for() {
        // `hosts_mcp` is hand-matched so it can be const; this is what stops the
        // two definitions drifting when an agent is added.
        let tmp = TempDir::new().unwrap();
        for agent in AgentId::all() {
            let from_table = !matches!(
                host_for(*agent, Scope::User, tmp.path(), tmp.path()),
                Host::None
            );
            assert_eq!(
                hosts_mcp(*agent),
                from_table,
                "{agent:?}: hosts_mcp disagrees with host_for"
            );
        }
    }

    #[test]
    fn every_agent_id_has_a_host_arm() {
        let tmp = TempDir::new().unwrap();
        for agent in AgentId::all() {
            // Exercises the match exhaustively; a new variant fails to compile
            // rather than silently defaulting.
            let _ = host_for(*agent, Scope::User, tmp.path(), tmp.path());
        }
    }
}
