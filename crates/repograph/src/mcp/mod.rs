//! The MCP tool surface behind `repograph mcp serve`.
//!
//! This module is a **sibling adapter** to `commands/`, not a layer above it.
//! Command handlers return `Result<(), RepographError>` and write to stdout as
//! they go, so there is no data value to intercept; both front-ends therefore
//! call the same `repograph-core` APIs and render the result their own way —
//! `output.rs` to a table or `--json`, this module to an MCP tool result.
//!
//! Payload shapes come from [`repograph_core::envelope`], which is what makes
//! "MCP output is identical to `--json` output" a property of a shared type
//! rather than two structs kept in sync by hand.
//!
//! # Why the tools are read-only
//!
//! The six tools here cover the entire read surface and nothing else. Mutating
//! the registry stays behind the `repograph-setup` skill's plan → confirm →
//! execute flow, for two reasons: the registry is the user's to manage, and a
//! uniformly read-only surface can be annotated `readOnlyHint`, which lets
//! clients auto-approve every call. A tool that raises a permission prompt on
//! each invocation gets avoided by agents and humans alike.
//!
//! # Blocking work on the runtime
//!
//! `repograph-core` is synchronous — `git2` and `SQLite` both block. The tools
//! below therefore do blocking work directly on the current-thread runtime that
//! `commands::mcp` owns. For a single-client stdio server this is correct: a
//! client waits for each response before issuing the next request, so there is
//! no concurrency to preserve. It does mean a long `find` stalls the read loop
//! for its duration, which is why `refresh_stale` keeps its mtime gate and the
//! `no_refresh` escape.

use std::path::PathBuf;

use repograph_core::{
    CONFIG_FILE_NAME, Config, Context, DoctorReport, FindEnvelope, ListEntry, ListEnvelope,
    RepoContext, RepoStatus, RepographError, SCHEMA_VERSION, Scope, StatusEnvelope, index_health,
    inspect, refresh_stale, search,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::prompt::host_home;
use crate::timestamp::now_rfc3339;

/// Default hit count for `repograph_find`, mirroring the CLI's `--limit`
/// default so the two surfaces rank and truncate identically.
const DEFAULT_FIND_LIMIT: usize = 10;

/// Instructions surfaced to the client at initialize time.
///
/// This is the one place the server gets to state a preference rather than a
/// capability, and it is deliberately short: the failure this whole change
/// addresses is an agent reaching for a generic file search instead of asking
/// the registry, so that is what the text corrects.
const SERVER_INSTRUCTIONS: &str = "repograph knows the user's own registered git repositories. \
Use these tools for questions that span repositories or name a project the user has registered — \
resolving a project name to a path, locating prior art across repos, or reporting state across \
all of them. Prefer repograph_find over a generic file search when the target repository is not \
known. For the working directory's own git state, use ordinary git instead.";

/// Optional workspace narrowing, shared by the tools that accept a scope.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScopeParams {
    /// Restrict to repos in this workspace. Omit to cover every registered repo.
    #[serde(default)]
    pub workspace: Option<String>,
}

/// Parameters for `repograph_switch`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SwitchParams {
    /// Name of the registered repo to resolve.
    pub name: String,
}

/// Parameters for `repograph_find`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindParams {
    /// What to look for — a description ("jwt refresh rotation") or a symbol name.
    pub query: String,
    /// Restrict to repos in this workspace. Omit to search every registered repo.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Maximum hits to return. Defaults to 10.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Skip the pre-search index refresh and query the index as-is. Faster, but
    /// results may miss uncommitted edits.
    #[serde(default)]
    pub no_refresh: Option<bool>,
}

/// The MCP server: the six read-only tools over one config and data directory.
///
/// Directories are resolved once at startup and held for the process lifetime,
/// but config is re-read on every call so registry edits are picked up without
/// restarting the server.
#[derive(Clone)]
pub struct RepographServer {
    config_dir: PathBuf,
    data_dir: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RepographServer {
    /// Build a server bound to the resolved config and data directories.
    #[must_use]
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "repograph_list",
        description = "List the user's registered git repositories with their absolute paths, \
                       descriptions, and tech stacks. Start here to learn which projects exist.",
        annotations(title = "List registered repos", read_only_hint = true)
    )]
    async fn list(&self, Parameters(params): Parameters<ScopeParams>) -> CallToolResult {
        Self::run_tool(|| {
            let config = Config::load(&self.config_dir)?;
            let scoped = scope_entries(&config, params.workspace.as_deref())?;
            let entries: Vec<ListEntry> = scoped
                .iter()
                .map(|(name, repo)| ListEntry::new(name, repo))
                .collect();
            to_value(&ListEnvelope { repos: &entries })
        })
    }

    #[tool(
        name = "repograph_status",
        description = "Report branch, upstream, ahead/behind counts, and working-tree cleanliness \
                       for every registered repo. Answers \"what is dirty / in flight across my \
                       projects\". Not for the current directory alone — use git for that.",
        annotations(title = "Cross-repo git status", read_only_hint = true)
    )]
    async fn status(&self, Parameters(params): Parameters<ScopeParams>) -> CallToolResult {
        Self::run_tool(|| {
            let config = Config::load(&self.config_dir)?;
            let scoped = scope_entries(&config, params.workspace.as_deref())?;
            let mut statuses: Vec<RepoStatus> = scoped
                .iter()
                .map(|(name, repo)| inspect(name, &repo.path, false))
                .collect();
            statuses.sort_by(|a, b| a.name.cmp(&b.name));
            for s in &statuses {
                if let Some(err) = &s.error {
                    tracing::warn!(repo = %s.name, err = %err, "mcp status: per-repo failure");
                }
            }
            to_value(&StatusEnvelope { repos: &statuses })
        })
    }

    #[tool(
        name = "repograph_context",
        description = "Load the agent instruction docs (CLAUDE.md, AGENTS.md, .cursor/rules, and \
                       the rest) for registered repos, inlined and ready to reason over. Use when \
                       you need another project's conventions. One call covers every repo — do \
                       not iterate.",
        annotations(title = "Aggregate repo agent docs", read_only_hint = true)
    )]
    async fn context(&self, Parameters(params): Parameters<ScopeParams>) -> CallToolResult {
        Self::run_tool(|| {
            let config = Config::load(&self.config_dir)?;
            let scoped = scope_entries(&config, params.workspace.as_deref())?;

            // The CLI refuses to build context without a configured agent
            // selection, and so must this tool. Returning an empty `agent_docs`
            // instead would tell the caller "this repo has no conventions" when
            // the truth is "repograph was never set up" — a silent wrong answer
            // in place of a loud, actionable one. The interactive repair the CLI
            // offers is unavailable here, so the message names the flag form.
            let agents = match config.agents() {
                Some(a) if !a.selected.is_empty() => a.selected.clone(),
                _ => {
                    return Err(RepographError::NeedsInit(
                        "agents not configured; run `repograph init` in an interactive shell, \
                         or `repograph init --no-prompt --agents <list>`"
                            .into(),
                    ));
                }
            };
            let mut repos: Vec<RepoContext> = scoped
                .iter()
                .map(|(name, repo)| RepoContext::build_one(name, &repo.path, &agents))
                .collect();
            repos.sort_by(|a, b| a.name.cmp(&b.name));
            for r in &repos {
                for w in &r.warnings {
                    tracing::warn!(repo = %r.name, warning = %w, "mcp context: per-repo warning");
                }
            }
            let scope = params
                .workspace
                .as_deref()
                .map_or(Scope::All, |name| Scope::Workspace {
                    name: name.to_string(),
                });
            to_value(&Context {
                schema_version: SCHEMA_VERSION,
                generated_at: now_rfc3339(),
                agents,
                scope,
                repos,
                warnings: Vec::new(),
            })
        })
    }

    #[tool(
        name = "repograph_switch",
        description = "Resolve a registered repo name to its absolute filesystem path. Use this \
                       to ground file operations on a project the user named, instead of guessing \
                       or searching the filesystem for it.",
        annotations(title = "Resolve repo name to path", read_only_hint = true)
    )]
    async fn switch(&self, Parameters(params): Parameters<SwitchParams>) -> CallToolResult {
        Self::run_tool(|| {
            let config = Config::load(&self.config_dir)?;
            let Some(repo) = config.repos().get(&params.name) else {
                return Err(RepographError::NotFound {
                    kind: "repo",
                    name: params.name.clone(),
                });
            };
            // Deliberately a path, not the CLI's `cd <quoted-path>` line: the
            // consumer here is a JSON-RPC client, not a shell.
            Ok(serde_json::json!({
                "name": params.name,
                "path": repo.path,
            }))
        })
    }

    #[tool(
        name = "repograph_find",
        description = "Search code across every registered repository by meaning or keyword. Use \
                       when the user says they solved something before, or wants prior art, and \
                       the repository is unknown. Prefer this over a generic file search for \
                       cross-repo questions.",
        annotations(title = "Cross-repo code search", read_only_hint = true)
    )]
    async fn find(&self, Parameters(params): Parameters<FindParams>) -> CallToolResult {
        // Semantic retrieval is a build-time feature and a slow path; the MCP
        // surface stays lexical so a tool call has predictable latency.
        const SEMANTIC: bool = false;

        Self::run_tool(|| {
            let config = Config::load(&self.config_dir)?;
            let scoped = scope_entries(&config, params.workspace.as_deref())?;
            let scope: Vec<(String, PathBuf)> = scoped
                .iter()
                .map(|(name, repo)| ((*name).clone(), repo.path.clone()))
                .collect();

            if !params.no_refresh.unwrap_or(false) {
                let mut progress = |_done: usize, _total: usize, name: &str| {
                    tracing::debug!(repo = %name, "mcp find: refreshing stale repo");
                };
                let refreshed = refresh_stale(&self.data_dir, &scope, SEMANTIC, &mut progress)?;
                if !refreshed.refreshed.is_empty() {
                    tracing::info!(
                        repos = refreshed.refreshed.len(),
                        files = refreshed.files_indexed,
                        "mcp find: auto-refreshed before search",
                    );
                }
            }

            let repos_filter: Vec<String> = if params.workspace.is_some() {
                scope.iter().map(|(name, _)| name.clone()).collect()
            } else {
                Vec::new()
            };
            let limit = params.limit.unwrap_or(DEFAULT_FIND_LIMIT);
            let outcome = search(
                &self.data_dir,
                &params.query,
                &repos_filter,
                limit,
                SEMANTIC,
            )?;
            if let Some(reason) = &outcome.degraded {
                tracing::warn!(reason = %reason, "mcp find: degraded to lexical");
            }
            to_value(&FindEnvelope::new(
                &params.query,
                &outcome.hits,
                outcome.semantic_used,
                outcome.degraded.as_deref(),
            ))
        })
    }

    #[tool(
        name = "repograph_doctor",
        description = "Health-check the registry: missing paths, stale search index, and agent \
                       artifact drift. Run this first when the user reports their setup behaving \
                       oddly, instead of guessing at the cause.",
        annotations(title = "Diagnose registry health", read_only_hint = true)
    )]
    async fn doctor(&self) -> CallToolResult {
        Self::run_tool(|| {
            let config_path = self.config_dir.join(CONFIG_FILE_NAME);
            let generated_at = now_rfc3339();
            let load = Config::load(&self.config_dir);

            let report = match &load {
                Ok(cfg) => DoctorReport::run(Ok(cfg), &config_path, generated_at),
                Err(err) => DoctorReport::run(Err(err), &config_path, generated_at),
            };

            // Mirror the CLI's assembly order exactly — index health, then the
            // artifact freshness check — so the payloads stay identical.
            let report = match &load {
                Ok(cfg) => {
                    let repos: Vec<(String, PathBuf)> = cfg
                        .repos()
                        .iter()
                        .map(|(name, repo)| (name.clone(), repo.path.clone()))
                        .collect();
                    let status = index_health(&self.data_dir, &repos)?;
                    report.with_index_check(&status)
                }
                Err(_) => report,
            };

            let report = match (&load, host_home(), std::env::current_dir()) {
                (Ok(cfg), Some(home), Ok(cwd)) => {
                    let selected = cfg.agents().map_or(&[][..], |a| a.selected.as_slice());
                    report.with_skill_artifact_check(selected, &home, &cwd)
                }
                _ => report,
            };

            to_value(&report)
        })
    }

    /// Run a tool body, converting a [`RepographError`] into a tool-level error
    /// result rather than a protocol error.
    ///
    /// MCP distinguishes the two: a protocol error is rendered opaquely by
    /// clients ("tool result missing due to internal error") and the message
    /// never reaches the user, whereas a tool-level error carries its content
    /// through. Every failure reachable here — an unknown repo name, a missing
    /// index, an unreadable config — is something the caller should read and
    /// act on, so all of them take the tool-level path. Returning `Ok` also
    /// keeps the connection alive: one bad call must not end the session.
    fn run_tool<F>(body: F) -> CallToolResult
    where
        F: FnOnce() -> Result<Value, RepographError>,
    {
        match body() {
            Ok(value) => CallToolResult::structured(value),
            Err(err) => {
                tracing::error!(err = ?err, "mcp: tool call failed");
                CallToolResult::structured_error(serde_json::json!({
                    "error": err.to_string(),
                    "exit_code": err.exit_code(),
                }))
            }
        }
    }
}

// `router = self.tool_router` overrides the macro default of
// `Self::tool_router()`, which would rebuild the whole router on every single
// tool call and dispatch. The router is built once in `new`.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for RepographServer {
    fn get_info(&self) -> ServerInfo {
        // Name and version are overwritten rather than taken from
        // `Implementation::from_build_env()`, which reads *rmcp's* crate
        // metadata and would announce this server to clients as "rmcp".
        let mut identity = Implementation::from_build_env();
        identity.name = env!("CARGO_PKG_NAME").to_string();
        identity.version = env!("CARGO_PKG_VERSION").to_string();

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(identity)
            .with_instructions(SERVER_INSTRUCTIONS)
    }
}

/// Resolve the repos a call applies to: a workspace's live members, or every
/// registered repo when no workspace is named.
///
/// Dangling workspace members are dropped here exactly as `list --workspace`
/// drops them; `doctor` is where dangling state is meant to surface.
fn scope_entries<'a>(
    config: &'a Config,
    workspace: Option<&str>,
) -> Result<Vec<(&'a String, &'a repograph_core::Repo)>, RepographError> {
    match workspace {
        Some(name) => {
            let (live, _dangling) = config.resolve_workspace(name)?;
            Ok(live)
        }
        None => Ok(config.repos().iter().collect()),
    }
}

/// Serialise an envelope into the `Value` a tool result carries.
fn to_value<T: serde::Serialize>(payload: &T) -> Result<Value, RepographError> {
    serde_json::to_value(payload).map_err(|e| RepographError::Io(std::io::Error::other(e)))
}
