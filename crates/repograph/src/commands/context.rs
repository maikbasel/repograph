//! `repograph context [<repos>...] [--workspace <name>] [--json]` — produce
//! the agent-facing payload for one or many registered repositories.
//!
//! Resolves an in-scope repo set, opens each repo via `git2` for its branch,
//! walks the agent registry's file patterns to inline matching files, and
//! emits either JSON (when `--json` or stdout is not a TTY) or Markdown (when
//! stdout is a TTY) on stdout. Per-repo and per-file failures surface as
//! inline warnings rather than aborts — the calling agent gets what we can
//! resolve, plus a stable list of what we couldn't.

use std::path::Path;

use clap::Parser;
use repograph_core::{Config, Context, RepoContext, RepographError, SCHEMA_VERSION, Scope};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::output::{OutputMode, render_context, with_progress};
use crate::prompt::ensure_agents_configured;

#[derive(Debug, Parser)]
pub struct Args {
    /// Restrict scope to specific registered repos by name. Mutually exclusive
    /// with `--workspace`. When omitted (and `--workspace` is omitted), every
    /// registered repo is in scope.
    #[arg(value_name = "NAME")]
    pub repos: Vec<String>,

    /// Restrict scope to members of the named workspace. Dangling members are
    /// silently skipped (parity with `list --workspace` and `status
    /// --workspace`). Mutually exclusive with positional repo names.
    #[arg(long, value_name = "NAME", conflicts_with = "repos")]
    pub workspace: Option<String>,

    /// Force JSON output regardless of TTY detection. TTY default is Markdown.
    #[arg(long)]
    pub json: bool,
}

/// Load config, gate on `[agents]`, resolve scope, build per-repo context in
/// parallel, render to stdout.
///
/// # Errors
///
/// Propagates [`RepographError`] for: malformed config (exit `1`), missing
/// `[agents]` in non-TTY (exit `2`), unknown workspace / repo name (exit `3`),
/// permission denied on config write during agent prompt (exit `4`).
#[tracing::instrument(skip(args, config_dir), fields(
    repos = ?args.repos,
    workspace = args.workspace.as_deref().unwrap_or("<none>"),
    json = args.json,
    config_dir = %config_dir.display(),
))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!(command = "context", "start");

    let mut config = Config::load(config_dir)?;
    ensure_agents_configured(&mut config, config_dir)?;

    let scope = scope_from_args(args);
    let targets = resolve_targets(&config, &scope)?;
    let agents = config
        .agents()
        .map(|a| a.selected.clone())
        .unwrap_or_default();

    let mode = OutputMode::detect(args.json);
    let label = |t: &Target| t.name.clone();
    let agents_for_workers = agents.clone();
    let repos: Vec<RepoContext> = with_progress(mode, &targets, label, |target| {
        RepoContext::build_one(&target.name, &target.path, &agents_for_workers)
    });
    let mut repos = repos;
    repos.sort_by(|a, b| a.name.cmp(&b.name));

    for r in &repos {
        for w in &r.warnings {
            tracing::warn!(repo = %r.name, warning = %w, "context: per-repo warning");
        }
    }

    let context = Context {
        schema_version: SCHEMA_VERSION,
        generated_at: now_rfc3339(),
        agents,
        scope,
        repos,
        warnings: Vec::new(),
    };

    let total_bytes: u64 = context
        .repos
        .iter()
        .flat_map(|r| r.agent_docs.iter())
        .flat_map(|d| d.files.iter())
        .map(|f| f.bytes)
        .sum();

    render_context(mode, &context)?;
    tracing::info!(
        repos = context.repos.len(),
        agents = context.agents.len(),
        bytes = total_bytes,
        "context built",
    );
    Ok(())
}

/// Per-target tuple passed to the worker; cloning the path means workers are
/// `Send + Sync` without borrowing from the config.
struct Target {
    name: String,
    path: std::path::PathBuf,
}

fn scope_from_args(args: &Args) -> Scope {
    args.workspace.as_ref().map_or_else(
        || {
            if args.repos.is_empty() {
                Scope::All
            } else {
                Scope::Repos {
                    repos: args.repos.clone(),
                }
            }
        },
        |name| Scope::Workspace { name: name.clone() },
    )
}

fn resolve_targets(config: &Config, scope: &Scope) -> Result<Vec<Target>, RepographError> {
    match scope {
        Scope::All => Ok(config
            .repos()
            .iter()
            .map(|(name, repo)| Target {
                name: name.clone(),
                path: repo.path.clone(),
            })
            .collect()),
        Scope::Workspace { name } => {
            let (live, _dangling) = config.resolve_workspace(name)?;
            Ok(live
                .into_iter()
                .map(|(member, repo)| Target {
                    name: member.clone(),
                    path: repo.path.clone(),
                })
                .collect())
        }
        Scope::Repos { repos } => {
            let mut seen = std::collections::BTreeSet::new();
            let mut targets = Vec::with_capacity(repos.len());
            for name in repos {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let repo =
                    config
                        .repos()
                        .get(name)
                        .ok_or_else(|| RepographError::NotFound {
                            kind: "repo",
                            name: name.clone(),
                        })?;
                targets.push(Target {
                    name: name.clone(),
                    path: repo.path.clone(),
                });
            }
            Ok(targets)
        }
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
