//! `repograph find "<query>" [--workspace <name>] [--limit <n>] [--semantic]
//! [--json]` — locate code across all registered repos by meaning or keyword.
//!
//! The intended use is cross-repo precedent: "I solved something like this
//! before, in a repo I can't name." Stdout carries the ranked hits (JSON
//! envelope when piped / `--json`, a table on TTY); diagnostics go to stderr.

use std::path::{Path, PathBuf};

use clap::Parser;
use repograph_core::{Config, RepographError, refresh_stale, search};

use crate::output::{OutputMode, render_hits};

/// Default number of hits returned when `--limit` is omitted.
const DEFAULT_LIMIT: usize = 10;

#[derive(Debug, Parser)]
pub struct Args {
    /// What to search for — a natural-language description ("jwt refresh token
    /// rotation") or an exact symbol name.
    #[arg(value_name = "QUERY")]
    pub query: String,

    /// Restrict the search to repos belonging to the named workspace. When
    /// omitted, all registered repos are searched.
    #[arg(long, value_name = "NAME")]
    pub workspace: Option<String>,

    /// Maximum number of hits to return.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_LIMIT)]
    pub limit: usize,

    /// Use semantic (embedding) retrieval in addition to keyword matching.
    /// Requires a build with the `semantic` feature and a semantic index;
    /// otherwise degrades to lexical with a notice on stderr.
    #[arg(long)]
    pub semantic: bool,

    /// Force JSON output regardless of TTY detection.
    #[arg(long)]
    pub json: bool,

    /// Skip the automatic pre-search index refresh and query the existing index
    /// as-is. Without this, `find` reindexes any in-scope repo whose working tree
    /// changed since it was last indexed (including uncommitted edits) before
    /// searching. With it, a missing index is an error (exit 3) rather than being
    /// built on demand.
    #[arg(long)]
    pub no_refresh: bool,
}

/// Resolve scope, run the hybrid search, and render the ranked hits.
///
/// # Errors
///
/// Returns [`RepographError::IndexMissing`] (exit 3) when no index has been
/// built or an unknown `--workspace` is given, and [`RepographError::Index`]
/// (exit 1) when the index is unreadable. Empty results are success (exit 0).
#[tracing::instrument(skip(args, config_dir, data_dir), fields(
    workspace = args.workspace.as_deref().unwrap_or("<all>"),
    limit = args.limit,
    semantic = args.semantic,
    json = args.json,
))]
pub fn run(args: &Args, config_dir: &Path, data_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("find: start");
    let config = Config::load(config_dir)?;
    let mode = OutputMode::detect(args.json);

    // The repos this query will reach, as (name, path). For a workspace scope
    // that is its live members; otherwise every registered repo.
    let scope = resolve_scope(&config, args.workspace.as_deref())?;

    // Auto-refresh (unless opted out): bring any in-scope repo whose working tree
    // changed — including uncommitted edits — up to date before querying, so
    // results reflect what is on disk now. Diagnostics on stderr only.
    if !args.no_refresh {
        let mut progress = |_done: usize, _total: usize, name: &str| {
            tracing::debug!(repo = %name, "find: refreshing stale repo");
        };
        let refreshed = refresh_stale(data_dir, &scope, args.semantic, &mut progress)?;
        if refreshed.refreshed.is_empty() {
            tracing::debug!("find: index already fresh");
        } else {
            tracing::info!(
                repos = refreshed.refreshed.len(),
                files = refreshed.files_indexed,
                "find: auto-refreshed before search",
            );
            eprintln!(
                "Refreshed {} repo(s) before search: {}.",
                refreshed.refreshed.len(),
                refreshed.refreshed.join(", ")
            );
        }
    }

    // A workspace scope filters results to its member names; the default scope
    // (all repos) passes an empty filter, which `search` reads as "no filter".
    let repos_filter: Vec<String> = if args.workspace.is_some() {
        scope.iter().map(|(name, _)| name.clone()).collect()
    } else {
        Vec::new()
    };

    let outcome = search(
        data_dir,
        &args.query,
        &repos_filter,
        args.limit,
        args.semantic,
    )?;

    if let Some(reason) = &outcome.degraded {
        tracing::warn!(reason = %reason, "find: semantic unavailable; used lexical");
        eprintln!("note: {reason}; showing keyword results");
    }

    render_hits(
        mode,
        &args.query,
        &outcome.hits,
        outcome.semantic_used,
        outcome.degraded.as_deref(),
    )?;

    tracing::info!(
        hits = outcome.hits.len(),
        semantic_used = outcome.semantic_used,
        "find: rendered",
    );
    Ok(())
}

/// Resolve the repos a query will reach as `(name, absolute_path)` pairs. With a
/// workspace, dangling members are skipped (parity with `index`'s scope
/// resolution). Used to bound the auto-refresh to exactly what `search` queries.
fn resolve_scope(
    config: &Config,
    workspace: Option<&str>,
) -> Result<Vec<(String, PathBuf)>, RepographError> {
    if let Some(ws) = workspace {
        let (live, _dangling) = config.resolve_workspace(ws)?;
        return Ok(live
            .into_iter()
            .map(|(name, repo)| (name.clone(), repo.path.clone()))
            .collect());
    }
    Ok(config
        .repos()
        .iter()
        .map(|(name, repo)| (name.clone(), repo.path.clone()))
        .collect())
}
