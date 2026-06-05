//! `repograph update [--check]` — receipt-aware in-place self-update.
//!
//! Installer/tarball installs (which carry a cargo-dist receipt) are upgraded in
//! place with checksum verification. Homebrew / `cargo install` builds carry no
//! receipt: the command changes nothing and points the user at their package
//! manager. The orchestration lives in [`crate::selfupdate`]; this layer is
//! presentation only — all status is written to stderr, stdout stays empty.

use std::io;

use clap::Parser;
use repograph_core::RepographError;

use crate::{output, selfupdate};

#[derive(Debug, Parser)]
pub struct Args {
    /// Report whether a newer version is available without installing it.
    #[arg(long)]
    pub check: bool,
}

/// Check for, and unless `--check` is set, install a newer `repograph`.
///
/// # Errors
///
/// Returns [`RepographError::UpdateFailed`] on a network, runtime, or
/// verification failure, or [`RepographError::PermissionDenied`] when the
/// running binary cannot be replaced.
#[tracing::instrument(skip(args), fields(check = args.check))]
pub fn run(args: &Args) -> Result<(), RepographError> {
    tracing::debug!(command = "update", check = args.check, "start");

    let outcome = selfupdate::run_update(args.check)?;

    let mut stderr = io::stderr().lock();
    output::render_update_outcome(&mut stderr, &outcome)?;

    tracing::info!(outcome = ?outcome, "update finished");
    Ok(())
}
