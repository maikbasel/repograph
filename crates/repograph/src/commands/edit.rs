//! `repograph edit <name>` — update a registered repository in place.
//!
//! Unlike remove-then-add, this preserves workspace memberships: renaming an
//! entry rewrites every workspace reference so groupings survive. At least one
//! change flag must be supplied; an empty invocation is a usage error.

use std::path::{Path, PathBuf};

use clap::Parser;
use repograph_core::{Config, RepoEdit, RepographError, validate_git_repo};

use crate::output::{self, Mutation, RepoConfirmation};

#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the registered repository to edit.
    pub name: String,

    /// Rename the entry. Workspace memberships are rewritten to the new name.
    #[arg(long = "name")]
    pub new_name: Option<String>,

    /// Set the description, or clear it when passed an empty string.
    #[arg(long)]
    pub description: Option<String>,

    /// Replace the technology tags wholesale (e.g. `--stack rust,cli`).
    #[arg(long, value_delimiter = ',')]
    pub stack: Option<Vec<String>>,

    /// Point the entry at a different local git repository path. Validated and
    /// stored canonicalized, exactly like `add`.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Emit a JSON confirmation of the updated entry to stdout.
    #[arg(long)]
    pub json: bool,
}

impl Args {
    /// Whether any change flag was supplied. An edit with none is a usage error.
    const fn has_change(&self) -> bool {
        self.new_name.is_some()
            || self.description.is_some()
            || self.stack.is_some()
            || self.path.is_some()
    }
}

/// Apply the requested in-place edits to the named repo.
///
/// # Errors
///
/// Returns [`RepographError::UsageError`] (exit 2) when no change flag is
/// supplied; propagates [`RepographError::NotFound`] (exit 3) for an unknown
/// name or a non-git `--path`, [`RepographError::Conflict`] (exit 5) for a
/// rename/path collision, and config load/save errors.
#[tracing::instrument(skip(args), fields(
    name = %args.name,
    config_dir = %config_dir.display(),
))]
pub fn run(args: Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("edit: start");

    if !args.has_change() {
        return Err(RepographError::UsageError(
            "edit requires at least one of --name, --description, --stack, --path".to_string(),
        ));
    }

    // Validate and canonicalize a new path up front (mirrors `add`); a non-git
    // path surfaces as NotFound (exit 3) before any config mutation.
    let path = match &args.path {
        Some(p) => Some(validate_git_repo(p)?),
        None => None,
    };

    let edit = RepoEdit {
        new_name: args.new_name.filter(|s| !s.is_empty()),
        // `Some(empty)` clears; `Some(text)` sets; `None` leaves unchanged.
        description: args.description.map(|s| (!s.is_empty()).then_some(s)),
        stack: args.stack,
        path,
    };

    let mut config = Config::load(config_dir)?;
    let (name, _repo) = config.edit_repo(&args.name, edit)?;
    config.save(config_dir)?;

    tracing::info!(repo = %name, "edited");

    if args.json {
        if let Some(repo) = config.repos().get(&name) {
            output::render_mutation(&Mutation::Edit {
                repo: RepoConfirmation::new(&name, repo),
            })?;
        }
    }
    Ok(())
}
