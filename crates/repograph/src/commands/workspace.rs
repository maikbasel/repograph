//! `repograph workspace …` — manage named groupings of registered repos.

use std::path::Path;

use clap::{Parser, Subcommand};
use repograph_core::{Config, RepographError, validate_workspace_name};

use crate::output::{OutputMode, render_workspace_show, render_workspaces};

#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: WorkspaceCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// Create an empty workspace.
    Create(CreateArgs),
    /// Delete a workspace. Registered repos are untouched.
    Rm(RmArgs),
    /// List the registered workspaces.
    Ls(LsArgs),
    /// Show one workspace's members, resolved against the repo registry.
    Show(ShowArgs),
    /// Attach one or more registered repos to a workspace.
    Add(MembershipArgs),
    /// Detach one or more repos from a workspace.
    Remove(MembershipArgs),
}

#[derive(Debug, Parser)]
pub struct CreateArgs {
    /// Workspace name (lowercase, alphanumeric + hyphen, max 63 chars).
    pub name: String,

    /// Optional human-readable description.
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(Debug, Parser)]
pub struct RmArgs {
    /// Name of the workspace to delete.
    pub name: String,
}

#[derive(Debug, Parser)]
pub struct LsArgs {
    /// Force JSON output regardless of TTY detection.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct ShowArgs {
    /// Name of the workspace to show.
    pub name: String,

    /// Force JSON output regardless of TTY detection.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct MembershipArgs {
    /// Name of the workspace to modify.
    pub workspace: String,

    /// One or more repo names to add or remove.
    #[arg(required = true)]
    pub repos: Vec<String>,
}

/// Dispatch the parsed `workspace` subcommand.
///
/// # Errors
///
/// Propagates [`RepographError`] from validation, config load/save, and the
/// individual subcommand handlers.
pub fn run(args: Args, config_dir: &Path) -> Result<(), RepographError> {
    match args.command {
        WorkspaceCommand::Create(a) => run_create(a, config_dir),
        WorkspaceCommand::Rm(a) => run_rm(&a, config_dir),
        WorkspaceCommand::Ls(a) => run_ls(&a, config_dir),
        WorkspaceCommand::Show(a) => run_show(&a, config_dir),
        WorkspaceCommand::Add(a) => run_add(&a, config_dir),
        WorkspaceCommand::Remove(a) => run_remove(&a, config_dir),
    }
}

#[tracing::instrument(skip(args), fields(
    name = %args.name,
    config_dir = %config_dir.display(),
))]
fn run_create(args: CreateArgs, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("workspace create: start");
    validate_workspace_name(&args.name)?;
    let mut config = Config::load(config_dir)?;
    config.create_workspace(args.name.clone(), args.description)?;
    config.save(config_dir)?;
    tracing::info!(workspace = %args.name, "created");
    Ok(())
}

#[tracing::instrument(skip(args), fields(
    name = %args.name,
    config_dir = %config_dir.display(),
))]
fn run_rm(args: &RmArgs, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("workspace rm: start");
    let mut config = Config::load(config_dir)?;
    config.remove_workspace(&args.name)?;
    config.save(config_dir)?;
    tracing::info!(workspace = %args.name, "removed");
    Ok(())
}

#[tracing::instrument(skip(args), fields(
    json = args.json,
    config_dir = %config_dir.display(),
))]
fn run_ls(args: &LsArgs, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("workspace ls: start");
    let config = Config::load(config_dir)?;
    let mode = OutputMode::detect(args.json);
    render_workspaces(mode, config.workspaces())?;
    tracing::info!(count = config.workspaces().len(), "listed");
    Ok(())
}

#[tracing::instrument(skip(args), fields(
    name = %args.name,
    json = args.json,
    config_dir = %config_dir.display(),
))]
fn run_show(args: &ShowArgs, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("workspace show: start");
    let config = Config::load(config_dir)?;
    let (live, dangling) = config.resolve_workspace(&args.name)?;
    for name in &dangling {
        tracing::warn!(
            workspace = %args.name,
            member = %name,
            "workspace references unregistered repo"
        );
    }
    let description = config
        .workspaces()
        .get(&args.name)
        .and_then(|w| w.description.as_deref());
    let mode = OutputMode::detect(args.json);
    render_workspace_show(mode, &args.name, description, &live, &dangling)?;
    tracing::info!(
        workspace = %args.name,
        live = live.len(),
        dangling = dangling.len(),
        "shown"
    );
    Ok(())
}

#[tracing::instrument(skip(args), fields(
    workspace = %args.workspace,
    config_dir = %config_dir.display(),
))]
fn run_add(args: &MembershipArgs, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("workspace add: start");
    let mut config = Config::load(config_dir)?;
    config.add_members(&args.workspace, &args.repos)?;
    config.save(config_dir)?;
    tracing::info!(
        workspace = %args.workspace,
        added = args.repos.len(),
        "members added"
    );
    Ok(())
}

#[tracing::instrument(skip(args), fields(
    workspace = %args.workspace,
    config_dir = %config_dir.display(),
))]
fn run_remove(args: &MembershipArgs, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("workspace remove: start");
    let mut config = Config::load(config_dir)?;
    config.remove_members(&args.workspace, &args.repos)?;
    config.save(config_dir)?;
    tracing::info!(
        workspace = %args.workspace,
        removed = args.repos.len(),
        "members removed"
    );
    Ok(())
}
