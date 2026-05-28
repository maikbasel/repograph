//! repograph CLI entrypoint.

mod commands;
mod output;
mod prompt;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use repograph_core::{Config, RepographError};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "repograph", version, about)]
pub(crate) struct Cli {
    /// Override the config directory. Resolution precedence:
    /// `--config-dir` > `REPOGRAPH_CONFIG_DIR` > platform default.
    #[arg(long, global = true, env = "REPOGRAPH_CONFIG_DIR", value_name = "PATH")]
    config_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Register a local git repository.
    Add(commands::add::Args),
    /// Generate static shell completions for bash, zsh, fish, powershell, or
    /// elvish. Writes the script to stdout for one-time install.
    Completions(commands::completions::Args),
    /// Aggregate per-repo agent docs (CLAUDE.md, AGENTS.md, .cursor/rules,
    /// CONVENTIONS.md, .windsurfrules, …) into a single payload — JSON by
    /// default for piping, Markdown when stdout is a TTY.
    Context(commands::context::Args),
    /// Run a read-only health check over the config and every registered repo.
    /// JSON when piped or `--json`; coloured table when stdout is a TTY.
    Doctor(commands::doctor::Args),
    /// Interactive setup — pick the agent toolchain(s) you use; optionally
    /// register a repo and assign it to a workspace.
    Init(commands::init::Args),
    /// List the registered repositories.
    List(commands::list::Args),
    /// Remove a registered repository by name.
    Remove(commands::remove::Args),
    /// Report working-tree, branch, and upstream state across registered repos.
    Status(commands::status::Args),
    /// Emit `cd <path>` for the named registered repo, eval-ready in any
    /// shell. Pair with the `rg-cd` shell function documented in the README.
    Switch(commands::switch::Args),
    /// Manage workspaces (named groupings of registered repositories).
    Workspace(commands::workspace::Args),
}

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    let config_dir = match resolve_config_dir(cli.config_dir, Config::default_dir()) {
        Ok(p) => p,
        Err(e) => return report(&e),
    };

    let result = match cli.command {
        Command::Add(args) => commands::add::run(args, &config_dir),
        Command::Completions(args) => commands::completions::run(&args),
        Command::Context(args) => commands::context::run(&args, &config_dir),
        Command::Doctor(args) => commands::doctor::run(&args, &config_dir),
        Command::Init(args) => commands::init::run(&args, &config_dir),
        Command::List(args) => commands::list::run(&args, &config_dir),
        Command::Remove(args) => commands::remove::run(&args, &config_dir),
        Command::Status(args) => commands::status::run(&args, &config_dir),
        Command::Switch(args) => commands::switch::run(&args, &config_dir),
        Command::Workspace(args) => commands::workspace::run(args, &config_dir),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => report(&e),
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .try_init();
}

/// Walk the precedence chain: `override` (CLI flag, possibly populated by
/// the `REPOGRAPH_CONFIG_DIR` env var via clap) > `default` (platform
/// default). Returns a usage error when neither is available.
fn resolve_config_dir(
    override_path: Option<PathBuf>,
    default: Option<PathBuf>,
) -> Result<PathBuf, RepographError> {
    if let Some(p) = override_path {
        return Ok(p);
    }
    default.ok_or_else(|| {
        RepographError::UsageError(
            "no config directory available; pass --config-dir or set REPOGRAPH_CONFIG_DIR"
                .to_string(),
        )
    })
}

fn report(err: &RepographError) -> ExitCode {
    // `doctor` rendered its report (the user-facing surface) before returning
    // this variant; the generic "repograph failed" line would be confusing
    // noise on top of an already-complete report.
    if !matches!(err, RepographError::DoctorErrorsFound { .. }) {
        tracing::error!(error = %err, "repograph failed");
    }
    ExitCode::from(err.exit_code())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn override_wins_over_default() {
        let p = resolve_config_dir(
            Some(PathBuf::from("/tmp/override")),
            Some(PathBuf::from("/tmp/default")),
        )
        .unwrap();
        assert_eq!(p, PathBuf::from("/tmp/override"));
    }

    #[test]
    fn default_used_when_no_override() {
        let p = resolve_config_dir(None, Some(PathBuf::from("/tmp/default"))).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/default"));
    }

    #[test]
    fn no_override_no_default_returns_usage_error_exit_1() {
        let err = resolve_config_dir(None, None).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        let msg = err.to_string();
        assert!(
            msg.contains("--config-dir"),
            "message guides user to --config-dir, got: {msg}"
        );
    }
}
