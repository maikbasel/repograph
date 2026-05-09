//! `repograph list [--json]` — render the registered repositories.

use std::path::Path;

use clap::Parser;
use repograph_core::{Config, RepographError};

use crate::output::{OutputMode, render_repos};

#[derive(Debug, Parser)]
pub struct Args {
    /// Force JSON output regardless of TTY detection.
    #[arg(long)]
    pub json: bool,
}

/// Load the config and render to stdout in the appropriate output mode.
///
/// # Errors
///
/// Propagates [`RepographError`] from config load and rendering.
#[tracing::instrument(skip(args), fields(
    json = args.json,
    config_dir = %config_dir.display(),
))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("list: start");
    let config = Config::load(config_dir)?;
    let mode = OutputMode::detect(args.json);
    render_repos(mode, config.repos())?;
    tracing::info!(count = config.repos().len(), "listed");
    Ok(())
}
