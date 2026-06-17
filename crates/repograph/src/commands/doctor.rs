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
use repograph_core::{CONFIG_FILE_NAME, Config, DoctorReport, RepographError, index_health};
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

    // Read-only freshness check for the installed skill artifacts. Resolves the
    // expected paths under the host home / cwd and compares the version stamp;
    // it never writes. Skipped when home is unresolvable or `[agents]` is absent
    // (an empty selection produces no findings).
    let report = match (&load, host_home(), std::env::current_dir()) {
        (Ok(cfg), Some(home), Ok(cwd)) => {
            let selected = cfg.agents().map_or(&[][..], |a| a.selected.as_slice());
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

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
