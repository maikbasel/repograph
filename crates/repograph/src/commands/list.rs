//! `repograph list [--json] [--workspace <name>]` — render the registered repositories.

use std::path::Path;

use clap::Parser;
use repograph_core::{Config, RepographError};

use crate::output::{OutputMode, render_repo_slice, render_repos};

#[derive(Debug, Parser)]
pub struct Args {
    /// Force JSON output regardless of TTY detection.
    #[arg(long)]
    pub json: bool,

    /// Restrict output to repos belonging to the named workspace. Dangling
    /// members are silently skipped — `workspace show` is where dangling
    /// state surfaces.
    #[arg(long, value_name = "NAME")]
    pub workspace: Option<String>,
}

/// Load the config and render to stdout in the appropriate output mode.
///
/// # Errors
///
/// Propagates [`RepographError`] from config load, workspace resolution, and
/// rendering.
#[tracing::instrument(skip(args), fields(
    json = args.json,
    workspace = args.workspace.as_deref().unwrap_or("<none>"),
    config_dir = %config_dir.display(),
))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("list: start");
    let config = Config::load(config_dir)?;
    let mode = OutputMode::detect(args.json);
    if let Some(name) = &args.workspace {
        let (live, _dangling) = config.resolve_workspace(name)?;
        render_repo_slice(mode, &live)?;
        tracing::info!(workspace = %name, count = live.len(), "listed (filtered)");
    } else {
        render_repos(mode, config.repos())?;
        tracing::info!(count = config.repos().len(), "listed");
    }
    Ok(())
}
