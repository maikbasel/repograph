//! `repograph index [--workspace <name>] [--semantic]` — build or refresh the
//! cross-repo search index over the git-tracked content of registered repos.
//!
//! The command has no stdout data payload (the index is a side effect); a human
//! summary and any warnings go to stderr, and the exit code is `0` on success.

use std::path::{Path, PathBuf};

use clap::Parser;
use is_terminal::IsTerminal;
use repograph_core::{Config, RepographError, build_index};

#[derive(Debug, Parser)]
pub struct Args {
    /// Restrict indexing to repos belonging to the named workspace. When
    /// omitted, every registered repo is indexed.
    #[arg(long, value_name = "NAME")]
    pub workspace: Option<String>,

    /// Also compute semantic embeddings (requires a build with the `semantic`
    /// feature). Without it, the index is lexical-only and this flag degrades
    /// with a notice on stderr.
    #[arg(long)]
    pub semantic: bool,
}

/// Resolve scope, build the index, and report a summary on stderr.
///
/// # Errors
///
/// Propagates [`RepographError`] for config load failures, an unknown
/// `--workspace`, or an index store failure.
#[tracing::instrument(skip(args, config_dir, data_dir), fields(
    workspace = args.workspace.as_deref().unwrap_or("<all>"),
    semantic = args.semantic,
))]
pub fn run(args: &Args, config_dir: &Path, data_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("index: start");
    let config = Config::load(config_dir)?;
    let repos = resolve_repos(&config, args.workspace.as_deref())?;

    if repos.is_empty() {
        eprintln!("Nothing to index — no repositories in scope. Register some with `repograph add`.");
        tracing::info!("index: empty scope");
        return Ok(());
    }

    let spinner = start_spinner(repos.len());
    let outcome = build_index(data_dir, &repos, args.semantic)?;
    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    if let Some(reason) = &outcome.degraded {
        tracing::warn!(reason = %reason, "semantic indexing unavailable; indexed lexically");
        eprintln!("note: semantic indexing unavailable ({reason}); indexed lexically");
    }

    if outcome.changed {
        eprintln!(
            "Indexed {indexed} repo(s){skipped}: {files} file(s) updated, {unchanged} unchanged, {purged} purged{sem}.",
            indexed = outcome.repos_indexed,
            skipped = skipped_phrase(outcome.repos_skipped),
            files = outcome.files_indexed,
            unchanged = outcome.files_unchanged,
            purged = outcome.files_purged,
            sem = if outcome.semantic { " (with embeddings)" } else { "" },
        );
    } else {
        eprintln!(
            "Index already up to date ({indexed} repo(s), {unchanged} file(s)).",
            indexed = outcome.repos_indexed,
            unchanged = outcome.files_unchanged,
        );
    }

    tracing::info!(
        repos = outcome.repos_indexed,
        files = outcome.files_indexed,
        purged = outcome.files_purged,
        "index: complete",
    );
    Ok(())
}

fn skipped_phrase(skipped: usize) -> String {
    if skipped == 0 {
        String::new()
    } else {
        format!(" ({skipped} skipped)")
    }
}

fn start_spinner(repo_count: usize) -> Option<indicatif::ProgressBar> {
    if !std::io::stderr().is_terminal() {
        return None;
    }
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(
        indicatif::ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
    );
    pb.set_message(format!("Indexing {repo_count} repo(s)…"));
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    Some(pb)
}

/// Resolve the repos to index as `(name, absolute_path)` pairs. With a
/// workspace, dangling members are skipped (parity with `status`/`list`).
fn resolve_repos(
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
