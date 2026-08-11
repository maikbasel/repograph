//! Bringing an existing install's host integration up to date after an upgrade.
//!
//! # Why this exists
//!
//! repograph ships through four channels, and three of them — Homebrew,
//! `cargo install`, and the shell installer — replace the binary without ever
//! running repograph code. Only `repograph update` does. So hooking "the
//! upgrade" would reach a minority of users, and everyone else would keep the
//! integration state their *previous* version left behind: no MCP registration,
//! and an artifact body written against an older `ARTIFACT_BODY_VERSION`.
//!
//! Instead, the new binary notices it is new. `[settings].setup_version` records
//! the version that last completed setup; when it differs from the running
//! version, the idempotent parts of setup run once and the stamp advances.
//!
//! # What keeps this from being invasive
//!
//! - It never makes a new decision. It repairs only what `init` already
//!   established — no new agents, no prompting, no scope changes. An install
//!   with no `[agents]` section is left completely alone.
//! - It only touches managed regions: artifacts go through the delimiter
//!   contract, registration merges one key into an existing server map.
//! - It cannot break the command that triggered it. Every failure is caught and
//!   logged to stderr, and it runs *after* the triggering command's own work.

use std::path::Path;

use repograph_core::agent_artifact::Scope;
use repograph_core::{
    AgentId, ArtifactResult, Config, RegistrationResult, Settings, mcp_registration,
    refresh_installed_artifacts, register_mcp,
};

use crate::prompt::host_home;

/// The version this binary stamps once reconciliation completes.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Run reconciliation if this binary is newer than the one that last set up.
///
/// Fail-silent by construction: this runs after a command has already produced
/// its output and must never change that command's outcome. Every error path
/// logs to stderr and returns.
pub fn run_if_stale(config_dir: &Path) {
    // A config that will not load is not this function's problem to report —
    // the command that just ran would already have surfaced it.
    let Ok(config) = Config::load(config_dir) else {
        return;
    };
    if !needs_reconcile(&config) {
        return;
    }
    // No agent selection means `init` was never completed. Reconciliation
    // repairs setup; it does not perform it.
    let Some(selected) = config.agents().map(|a| a.selected.clone()) else {
        return;
    };
    if selected.is_empty() {
        // Explicitly opted out of agent docs at init. Still stamp, so this
        // check does not re-run on every subsequent command.
        stamp(config_dir, config);
        return;
    }

    let (Some(home), Ok(cwd)) = (host_home(), std::env::current_dir()) else {
        tracing::debug!("reconcile: home or cwd unresolvable; skipping");
        return;
    };

    tracing::info!(
        from = config
            .settings()
            .and_then(|s| s.setup_version.as_deref())
            .unwrap_or("<unset>"),
        to = CURRENT_VERSION,
        "reconcile: upgrading host integration",
    );
    eprintln!("repograph {CURRENT_VERSION}: updating agent integration…");

    let refreshed = refresh_installed_artifacts(&selected, &home, &cwd);
    log_artifact_results(&refreshed);

    let registered = register_all(&selected, &home, &cwd);
    log_registration_results(&registered);

    stamp(config_dir, config);
}

/// Has this binary already reconciled for its own version?
fn needs_reconcile(config: &Config) -> bool {
    config
        .settings()
        .and_then(|s| s.setup_version.as_deref())
        .is_none_or(|stamped| stamped != CURRENT_VERSION)
}

/// Record the running version as the completed setup version.
///
/// Written even when individual steps failed: the alternative is retrying the
/// same failing work after every command, which turns one stderr line into an
/// unbounded stream. `doctor` is where persistent failures surface.
fn stamp(config_dir: &Path, mut config: Config) {
    let mut settings = config.settings().cloned().unwrap_or_default();
    settings.setup_version = Some(CURRENT_VERSION.to_string());
    config.set_settings(Some(settings));
    if let Err(e) = config.save(config_dir) {
        tracing::warn!(err = %e, "reconcile: could not persist setup_version stamp");
    }
}

/// Write the current version stamp into a config being saved by `init`.
///
/// A fresh install has just done the work reconciliation would do, so stamping
/// here stops it running again on the very next command.
pub fn stamp_settings(settings: Option<&Settings>) -> Settings {
    let mut settings = settings.cloned().unwrap_or_default();
    settings.setup_version = Some(CURRENT_VERSION.to_string());
    settings
}

/// Register the MCP server for every selected agent that hosts one, at an
/// explicit scope.
///
/// `init` calls this with the scope the user chose, so a `--scope project`
/// install registers under the project and never touches the home directory.
/// Getting this wrong is not a cosmetic bug: it writes to a location the user
/// did not ask for.
#[must_use]
pub fn register_all_at(
    selected: &[AgentId],
    scope: Scope,
    home: &Path,
    cwd: &Path,
) -> Vec<RegistrationResult> {
    let Ok(exe) = std::env::current_exe() else {
        tracing::warn!("reconcile: current executable path unresolvable; skipping registration");
        return Vec::new();
    };
    selected
        .iter()
        .map(|&agent| register_mcp(agent, scope, home, cwd, &exe))
        .collect()
}

/// Register for every selected agent, inferring the scope from where each is
/// already registered.
///
/// Used by reconciliation and `doctor --fix`, which repair an existing setup
/// and have no `--scope` to go on. Re-registering where the entry already lives
/// stops a project-scope user silently acquiring a second user-scope entry;
/// absent any entry, user scope is the sensible default because it applies
/// wherever the user happens to be working.
#[must_use]
pub fn register_all(selected: &[AgentId], home: &Path, cwd: &Path) -> Vec<RegistrationResult> {
    let Ok(exe) = std::env::current_exe() else {
        tracing::warn!("reconcile: current executable path unresolvable; skipping registration");
        return Vec::new();
    };
    selected
        .iter()
        .map(|&agent| {
            let scope = mcp_registration::registered_scope(agent, home, cwd).unwrap_or(Scope::User);
            register_mcp(agent, scope, home, cwd, &exe)
        })
        .collect()
}

/// Report artifact refresh outcomes on stderr, per the output contract.
pub fn log_artifact_results(results: &[ArtifactResult]) {
    for r in results {
        match r {
            ArtifactResult::Written { agent, path, .. } => {
                tracing::info!(agent = agent.as_str(), path = %path.display(), "artifact refreshed");
                eprintln!("  refreshed {}", path.display());
            }
            ArtifactResult::Unchanged { .. } | ArtifactResult::Skipped { .. } => {}
            ArtifactResult::Failed { agent, error, .. } => {
                tracing::warn!(agent = agent.as_str(), err = ?error, "artifact refresh failed");
                eprintln!(
                    "  could not refresh artifact for {}: {error}",
                    agent.as_str()
                );
            }
        }
    }
}

/// Report MCP registration outcomes on stderr, per the output contract.
pub fn log_registration_results(results: &[RegistrationResult]) {
    for r in results {
        match r {
            RegistrationResult::Written { agent, target } => {
                tracing::info!(agent = agent.as_str(), %target, "mcp server registered");
                eprintln!("  registered MCP server for {} ({target})", agent.as_str());
            }
            RegistrationResult::Unchanged { agent, target } => {
                tracing::debug!(agent = agent.as_str(), %target, "mcp registration already current");
            }
            RegistrationResult::Skipped { agent, reason } => {
                tracing::debug!(agent = agent.as_str(), reason, "mcp registration skipped");
            }
            RegistrationResult::Failed { agent, error } => {
                tracing::warn!(agent = agent.as_str(), %error, "mcp registration failed");
                eprintln!(
                    "  could not register MCP server for {}: {error}",
                    agent.as_str()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use repograph_core::Agents;
    use tempfile::TempDir;

    fn config_with(agents: Option<Agents>, stamp: Option<&str>) -> Config {
        let mut cfg = Config::default();
        cfg.set_agents(agents);
        if let Some(v) = stamp {
            cfg.set_settings(Some(Settings {
                setup_version: Some(v.to_string()),
                ..Default::default()
            }));
        }
        cfg
    }

    #[test]
    fn an_unstamped_config_needs_reconciliation() {
        assert!(needs_reconcile(&config_with(None, None)));
    }

    #[test]
    fn an_older_stamp_needs_reconciliation() {
        assert!(needs_reconcile(&config_with(None, Some("0.0.1"))));
    }

    #[test]
    fn a_current_stamp_does_not() {
        assert!(!needs_reconcile(&config_with(None, Some(CURRENT_VERSION))));
    }

    #[test]
    fn an_install_without_agents_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let cfg = config_with(None, Some("0.0.1"));
        cfg.save(tmp.path()).unwrap();

        run_if_stale(tmp.path());

        // No `[agents]` means init never completed; nothing is written, not
        // even the stamp, so a later `init` still looks like a first run.
        let reloaded = Config::load(tmp.path()).unwrap();
        assert_eq!(
            reloaded.settings().and_then(|s| s.setup_version.as_deref()),
            Some("0.0.1"),
            "an unconfigured install must not be touched"
        );
    }

    #[test]
    fn an_empty_selection_is_stamped_so_it_stops_re_checking() {
        let tmp = TempDir::new().unwrap();
        let cfg = config_with(Some(Agents { selected: vec![] }), Some("0.0.1"));
        cfg.save(tmp.path()).unwrap();

        run_if_stale(tmp.path());

        let reloaded = Config::load(tmp.path()).unwrap();
        assert_eq!(
            reloaded.settings().and_then(|s| s.setup_version.as_deref()),
            Some(CURRENT_VERSION),
        );
    }

    #[test]
    fn stamp_settings_preserves_existing_fields() {
        let existing = Settings {
            projects_root: Some(std::path::PathBuf::from("/code")),
            setup_version: None,
        };
        let stamped = stamp_settings(Some(&existing));
        assert_eq!(
            stamped.projects_root,
            Some(std::path::PathBuf::from("/code"))
        );
        assert_eq!(stamped.setup_version.as_deref(), Some(CURRENT_VERSION));
    }
}
