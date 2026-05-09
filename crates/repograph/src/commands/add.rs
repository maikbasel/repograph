//! `repograph add <path>` — register a local git repository.

use std::path::{Path, PathBuf};

use clap::Parser;
use repograph_core::{Config, Repo, RepographError, validate_git_repo};

#[derive(Debug, Parser)]
pub struct Args {
    /// Path to a local git repository.
    pub path: PathBuf,

    /// Override the auto-derived name (defaults to the path's basename).
    #[arg(long)]
    pub name: Option<String>,

    /// Optional human-readable description.
    #[arg(long)]
    pub description: Option<String>,

    /// Comma-separated technology tags (e.g. `--stack rust,cli`).
    #[arg(long, value_delimiter = ',')]
    pub stack: Vec<String>,
}

/// Register the repo at `args.path` into the config under `args.name` (or
/// the path basename when omitted).
///
/// # Errors
///
/// Propagates [`RepographError`] from path validation, config load/save, and
/// uniqueness checks. The binary maps these to documented exit codes.
#[tracing::instrument(skip(args), fields(
    name = args.name.as_deref().unwrap_or("<inferred>"),
    path = %args.path.display(),
    config_dir = %config_dir.display(),
))]
pub fn run(args: Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("add: start");

    let canonical = validate_git_repo(&args.path)?;
    let name = match args.name {
        Some(n) if !n.is_empty() => n,
        _ => derive_name(&canonical)?,
    };

    let mut config = Config::load(config_dir)?;
    let repo = Repo {
        path: canonical,
        description: args.description.filter(|s| !s.is_empty()),
        stack: args.stack,
    };
    config.add_repo(name.clone(), repo)?;
    config.save(config_dir)?;

    tracing::info!(repo = %name, "registered");
    Ok(())
}

fn derive_name(path: &Path) -> Result<String, RepographError> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| RepographError::UsageError(format!(
            "could not derive a name from path '{}'; pass --name",
            path.display()
        )))
}
