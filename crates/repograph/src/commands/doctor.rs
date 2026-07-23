//! `repograph doctor [--json]` — read-only health check report.
//!
//! Loads the config, runs the full `doctor::Check` catalog, and emits the
//! report — `comfy-table` on TTY, single-line JSON when piped or `--json`.
//! Exit `0` when every finding is `ok` or `warn`; exit `1` when at least one
//! finding is `error`; exit `4` when the config file exists but cannot be read
//! due to permission denied.

use std::io;
use std::path::Path;

use std::path::PathBuf;

use clap::Parser;
use repograph_core::{
    ArtifactResult, CONFIG_FILE_NAME, Config, DoctorReport, RepographError, index_health,
    refresh_installed_artifacts,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::output::{OutputMode, render_doctor};
use crate::prompt::host_home;

#[derive(Debug, Parser)]
pub struct Args {
    /// Force JSON output regardless of TTY detection. TTY default is a
    /// `comfy-table` summary plus a `<N> ok · <M> warn · <K> error` footer.
    #[arg(long)]
    pub json: bool,

    /// Refresh stale or un-spliced skill artifacts in place before reporting.
    /// Re-runs the installer for every managed artifact that already exists
    /// (any scope), bringing its version stamp current and splicing the block
    /// into a shared file that lacks it — content outside the block is
    /// preserved. Never creates a missing artifact (that needs `init`, which
    /// chooses a scope). The rendered report reflects the post-fix state.
    #[arg(long)]
    pub fix: bool,
}

/// Build the doctor report, render it to stdout, signal exit `1` via
/// [`RepographError::DoctorErrorsFound`] when any error finding is present.
///
/// # Errors
///
/// Returns [`RepographError::Io`] / [`RepographError::PermissionDenied`]
/// (exit `4`) when the config file exists but the process cannot read it;
/// [`RepographError::DoctorErrorsFound`] (exit `1`) when the report contains
/// at least one error finding.
#[tracing::instrument(skip(args, config_dir, data_dir), fields(json = args.json))]
pub fn run(args: &Args, config_dir: &Path, data_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!(command = "doctor", json = args.json, "start");

    let mode = OutputMode::detect(args.json);
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    let generated_at = now_rfc3339();

    let load = Config::load(config_dir);
    if let Err(RepographError::Io(ref e)) = load {
        if e.kind() == io::ErrorKind::PermissionDenied {
            return Err(load
                .err()
                .unwrap_or_else(|| RepographError::Io(io::Error::other("permission denied"))));
        }
    }

    let report = match &load {
        Ok(cfg) => DoctorReport::run(Ok(cfg), &config_path, generated_at),
        Err(err) => DoctorReport::run(Err(err), &config_path, generated_at),
    };

    // Append the search-index health finding. Only meaningful when the config
    // loaded (we need the repo list to judge staleness); on a load error the
    // report already short-circuited, so skip it there.
    let report = match &load {
        Ok(cfg) => {
            let repos: Vec<(String, PathBuf)> = cfg
                .repos()
                .iter()
                .map(|(name, repo)| (name.clone(), repo.path.clone()))
                .collect();
            let status = index_health(data_dir, &repos)?;
            report.with_index_check(&status)
        }
        Err(_) => report,
    };

    // Optionally refresh installed skill artifacts in place (`--fix`), then run
    // the read-only freshness check. Both resolve paths under host home / cwd
    // and are skipped when home is unresolvable or `[agents]` is absent (an
    // empty selection produces no findings and nothing to fix). The refresh
    // runs *before* the check so the rendered report shows the post-fix state.
    let report = match (&load, host_home(), std::env::current_dir()) {
        (Ok(cfg), Some(home), Ok(cwd)) => {
            let selected = cfg.agents().map_or(&[][..], |a| a.selected.as_slice());
            if args.fix {
                let fixed = refresh_installed_artifacts(selected, &home, &cwd);
                log_fix_results(&fixed);
            }
            report.with_skill_artifact_check(selected, &home, &cwd)
        }
        _ => report,
    };

    render_doctor(mode, &report)?;

    if mode == OutputMode::Tty && report.summary.error > 0 {
        eprintln!("→ run `repograph doctor --json | jq` for machine-readable detail");
    }

    tracing::info!(
        ok = report.summary.ok,
        warn = report.summary.warn,
        error = report.summary.error,
        total = report.summary.total,
        "doctor complete",
    );

    if report.summary.error > 0 {
        return Err(RepographError::DoctorErrorsFound {
            count: report.summary.error,
        });
    }
    Ok(())
}

/// Emit one stderr line per artifact `--fix` touched, per the logging contract
/// (stdout stays reserved for the report). A `Written` result is a real
/// refresh; `Unchanged` means it was already current; `Failed` is surfaced as a
/// warning without aborting the report.
fn log_fix_results(results: &[ArtifactResult]) {
    for r in results {
        match r {
            ArtifactResult::Written { agent, path, .. } => {
                tracing::info!(agent = agent.as_str(), path = %path.display(), "artifact refreshed");
                eprintln!("refreshed {}", path.display());
            }
            ArtifactResult::Unchanged { agent, path, .. } => {
                tracing::debug!(agent = agent.as_str(), path = %path.display(), "artifact already current");
            }
            ArtifactResult::Failed { agent, error, .. } => {
                tracing::warn!(agent = agent.as_str(), err = ?error, "artifact refresh failed");
                eprintln!("could not refresh artifact for {}: {error}", agent.as_str());
            }
            // `refresh_installed_artifacts` gates on `has_artifact_writer`, so a
            // Skipped result never reaches here.
            ArtifactResult::Skipped { .. } => {}
        }
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
