//! `repograph completions <shell>` — emit a static completion script.
//!
//! Introspects the live `Cli` struct via `clap::CommandFactory` so the
//! generated script reflects the exact subcommand and flag surface. Drift is
//! structurally impossible — if a new flag exists in code, it appears in the
//! completion script the next time the user regenerates.

use std::io;

use clap::{CommandFactory, Parser};
use clap_complete::Shell;
use repograph_core::RepographError;

#[derive(Debug, Parser)]
pub struct Args {
    /// Target shell. One of `bash`, `zsh`, `fish`, `powershell`, `elvish`.
    #[arg(value_name = "SHELL")]
    pub shell: Shell,
}

/// Generate completions for `args.shell` and write to stdout.
///
/// # Errors
///
/// Returns [`RepographError::Io`] on stdout write failure.
#[tracing::instrument(skip(args), fields(shell = ?args.shell))]
pub fn run(args: &Args) -> Result<(), RepographError> {
    tracing::debug!(command = "completions", shell = ?args.shell, "start");

    let mut cmd = crate::Cli::command();
    let mut stdout = io::stdout().lock();
    clap_complete::generate(args.shell, &mut cmd, "repograph", &mut stdout);

    tracing::info!(shell = ?args.shell, "completions generated");
    Ok(())
}
