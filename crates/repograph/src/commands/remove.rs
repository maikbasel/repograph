//! `repograph remove <name>` — deregister a repository.

use std::path::Path;

use clap::Parser;
use repograph_core::{Config, RepographError};

#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the registered repository to remove.
    pub name: String,
}

/// Remove the named repository from the config.
///
/// # Errors
///
/// Propagates [`RepographError::NotFound`] when the name is not registered,
/// and other errors from config load/save.
#[tracing::instrument(skip(args), fields(
    name = %args.name,
    config_dir = %config_dir.display(),
))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("remove: start");
    let mut config = Config::load(config_dir)?;
    config.remove_repo(&args.name)?;
    config.save(config_dir)?;
    tracing::info!(repo = %args.name, "removed");
    Ok(())
}
