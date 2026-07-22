//! repograph CLI entrypoint.

mod commands;
mod output;
mod prompt;
mod selfupdate;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use is_terminal::IsTerminal;
use repograph_core::{Config, RepographError};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "repograph", version, about)]
pub(crate) struct Cli {
    /// Override the config directory. Resolution precedence:
    /// `--config-dir` > `REPOGRAPH_CONFIG_DIR` > platform default.
    #[arg(long, global = true, env = "REPOGRAPH_CONFIG_DIR", value_name = "PATH")]
    config_dir: Option<PathBuf>,

    /// Override the data directory (where the search index lives). Resolution
    /// precedence: `--data-dir` > `REPOGRAPH_DATA_DIR` > platform default.
    #[arg(long, global = true, env = "REPOGRAPH_DATA_DIR", value_name = "PATH")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
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
    /// Update a registered repository in place — rename, retag, re-describe, or
    /// repoint its path. Renames preserve workspace memberships.
    Edit(commands::edit::Args),
    /// Find code across all registered repos by meaning or keyword — locate a
    /// reference implementation when you're not sure which repo it's in.
    Find(commands::find::Args),
    /// Build or refresh the cross-repo search index. Git-aware and incremental:
    /// only changed files are re-processed.
    Index(commands::index::Args),
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
    /// Update repograph in place (installer/tarball installs) or report the
    /// upgrade command for Homebrew / `cargo install` builds. `--check` reports
    /// availability without installing.
    Update(commands::update::Args),
    /// Manage workspaces (named groupings of registered repositories).
    Workspace(commands::workspace::Args),
}

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    // No subcommand: print help and still surface the update nudge, matching the
    // npm / gh / cargo convention. Treated as success (exit 0), not a usage error.
    let Some(command) = cli.command else {
        let _ = Cli::command().print_help();
        selfupdate::notify(false);
        return ExitCode::SUCCESS;
    };

    let config_dir = match resolve_config_dir(cli.config_dir, Config::default_dir()) {
        Ok(p) => p,
        Err(e) => return report(&e),
    };

    let command_is_update = matches!(command, Command::Update(_));

    // The data dir is only needed by index/find/doctor; resolve lazily so a
    // platform with no data dir still runs every other command.
    let data_dir = || resolve_data_dir(cli.data_dir.clone(), default_data_dir());

    let result = match command {
        Command::Add(args) => commands::add::run(args, &config_dir),
        Command::Completions(args) => commands::completions::run(&args),
        Command::Context(args) => commands::context::run(&args, &config_dir),
        Command::Doctor(args) => {
            data_dir().and_then(|d| commands::doctor::run(&args, &config_dir, &d))
        }
        Command::Edit(args) => commands::edit::run(args, &config_dir),
        Command::Find(args) => data_dir().and_then(|d| commands::find::run(&args, &config_dir, &d)),
        Command::Index(args) => {
            data_dir().and_then(|d| commands::index::run(&args, &config_dir, &d))
        }
        Command::Init(args) => commands::init::run(&args, &config_dir),
        Command::List(args) => commands::list::run(&args, &config_dir),
        Command::Remove(args) => commands::remove::run(&args, &config_dir),
        Command::Status(args) => commands::status::run(&args, &config_dir),
        Command::Switch(args) => commands::switch::run(&args, &config_dir),
        Command::Update(args) => commands::update::run(&args),
        Command::Workspace(args) => commands::workspace::run(args, &config_dir),
    };

    let succeeded = result.is_ok();
    let exit = match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => report(&e),
    };

    // Passive update nudge — runs after the command's work, never alters the
    // exit code, and is fully gated + fail-silent inside `notify`. Suppressed on
    // a failed command so the nudge never stacks on top of an error message.
    if succeeded {
        selfupdate::notify(command_is_update);
    }

    exit
}

fn init_tracing() {
    // Default level is TTY-aware: when stderr is a terminal, an interactive
    // human is driving the CLI and the cliclack prompt UI shares stderr, so
    // info-level diagnostics would shred the wizard — default to `warn`. When
    // stderr is piped (CI, agents, log capture), keep `info` so the audit
    // trail is preserved without polluting stdout's data contract. `RUST_LOG`
    // overrides either default.
    let default_level = if std::io::stderr().is_terminal() {
        "warn"
    } else {
        "info"
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
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

/// Platform-default data directory: `dirs::data_dir() / "repograph"`. `None`
/// when no platform default exists; the binary surfaces this as a usage error
/// guiding the user to `--data-dir`.
fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("repograph"))
}

/// Walk the precedence chain for the data directory: `override` (CLI flag or
/// `REPOGRAPH_DATA_DIR` via clap) > platform default. Returns a usage error
/// when neither is available.
fn resolve_data_dir(
    override_path: Option<PathBuf>,
    default: Option<PathBuf>,
) -> Result<PathBuf, RepographError> {
    if let Some(p) = override_path {
        return Ok(p);
    }
    default.ok_or_else(|| {
        RepographError::UsageError(
            "no data directory available; pass --data-dir or set REPOGRAPH_DATA_DIR".to_string(),
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
    fn no_override_no_default_returns_usage_error_exit_2() {
        let err = resolve_config_dir(None, None).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let msg = err.to_string();
        assert!(
            msg.contains("--config-dir"),
            "message guides user to --config-dir, got: {msg}"
        );
    }
}
