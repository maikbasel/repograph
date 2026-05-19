//! `repograph status [<names>...] [--workspace <name>] [--json] [--fetch]` —
//! report working-tree, branch, and upstream state for one or many registered
//! repositories. Read-only consumer of `registry-core` and `workspace-support`.

use std::path::Path;

use clap::Parser;
use repograph_core::{Config, RepoState, RepoStatus, RepographError, inspect};

use crate::output::{OutputMode, render_statuses, with_progress};

#[derive(Debug, Parser)]
pub struct Args {
    /// Restrict output to specific registered repos by name. Mutually exclusive
    /// with `--workspace`. When omitted (and `--workspace` is omitted), all
    /// registered repos are scanned in alphabetical order.
    #[arg(value_name = "NAME")]
    pub names: Vec<String>,

    /// Restrict output to repos belonging to the named workspace. Dangling
    /// members are silently skipped (parity with `list --workspace`).
    /// Mutually exclusive with positional repo names.
    #[arg(long, value_name = "NAME", conflicts_with = "names")]
    pub workspace: Option<String>,

    /// Force JSON output regardless of TTY detection.
    #[arg(long)]
    pub json: bool,

    /// Opt-in: run `git fetch` against each repo's upstream remote before
    /// computing ahead/behind. Off by default — `status` is zero-network
    /// unless this flag is passed.
    #[arg(long)]
    pub fetch: bool,
}

/// Load config, resolve scope, fan repos out across `rayon`, render results.
///
/// # Errors
///
/// Propagates [`RepographError`] for config load failures, the names XOR
/// workspace usage rule, unknown positional names, unknown workspace names,
/// and the single-explicit-name-points-at-broken-repo case.
#[tracing::instrument(skip(args), fields(
    names = ?args.names,
    workspace = args.workspace.as_deref().unwrap_or("<none>"),
    json = args.json,
    fetch = args.fetch,
    config_dir = %config_dir.display(),
))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("status: start");

    let config = Config::load(config_dir)?;
    let mode = OutputMode::detect(args.json);

    let targets = resolve_targets(&config, args)?;

    if targets.is_empty() {
        render_statuses(mode, &[])?;
        tracing::info!(count = 0, "status: empty scope");
        return Ok(());
    }

    let fetch = args.fetch;
    let mut statuses: Vec<RepoStatus> = with_progress(
        mode,
        &targets,
        |t| t.name.clone(),
        |target| inspect(&target.name, &target.path, fetch),
    );
    statuses.sort_by(|a, b| a.name.cmp(&b.name));

    // Per-row warnings: surface drift via stderr. Detached HEADs and per-repo
    // errors both log here so the user sees them in TTY mode alongside the table.
    for s in &statuses {
        if let Some(err) = &s.error {
            tracing::warn!(repo = %s.name, err = %err, "status: per-repo failure");
        }
        if let Some(sha) = &s.detached_sha {
            tracing::warn!(repo = %s.name, sha = %sha, "status: detached HEAD");
        }
    }

    // Single-explicit-name semantics: if exactly one positional name was given
    // and that repo is missing, return NotFound (exit 3) without rendering.
    if args.names.len() == 1
        && args.workspace.is_none()
        && let Some(only) = statuses.first()
        && only.state == RepoState::Missing
    {
        return Err(RepographError::NotFound {
            kind: "repo",
            name: only.name.clone(),
        });
    }

    render_statuses(mode, &statuses)?;
    tracing::info!(count = statuses.len(), "status: rendered");
    Ok(())
}

struct Target {
    name: String,
    path: std::path::PathBuf,
}

fn resolve_targets(config: &Config, args: &Args) -> Result<Vec<Target>, RepographError> {
    if let Some(ws) = &args.workspace {
        let (live, _dangling) = config.resolve_workspace(ws)?;
        return Ok(live
            .into_iter()
            .map(|(name, repo)| Target {
                name: name.clone(),
                path: repo.path.clone(),
            })
            .collect());
    }

    if args.names.is_empty() {
        return Ok(config
            .repos()
            .iter()
            .map(|(name, repo)| Target {
                name: name.clone(),
                path: repo.path.clone(),
            })
            .collect());
    }

    // Positional names: validate each against the registry, dedupe.
    let mut seen = std::collections::BTreeSet::new();
    let mut targets = Vec::with_capacity(args.names.len());
    for name in &args.names {
        if !seen.insert(name.clone()) {
            continue;
        }
        let repo = config
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
