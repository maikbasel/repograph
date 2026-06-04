//! Self-update machinery shared by the `repograph update` command and the
//! passive update notifier.
//!
//! Both surfaces resolve "latest" from the same place — GitHub Releases for
//! `maikbasel/repograph` — so the nudge and the actual upgrade can never
//! disagree. The network query is driven through `axoupdater` on a private
//! current-thread tokio runtime, keeping the rest of the CLI synchronous.
//!
//! The pure decision logic (`is_newer`, `should_notify`, `cache_is_fresh`) and
//! the disposable on-disk throttle cache live here as small, independently
//! testable units; `notify` composes them into the post-command hook.

use std::error::Error;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use axoupdater::{AxoUpdater, AxoupdateError, ReleaseSource, ReleaseSourceType, Version};
use repograph_core::RepographError;
use serde::{Deserialize, Serialize};

/// Outcome of a `repograph update` run, rendered into user-facing stderr text
/// by the command layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The binary was upgraded in place.
    Updated { from: Option<String>, to: String },
    /// The running build is already the latest.
    AlreadyCurrent,
    /// `--check` only: a newer version exists but was not installed.
    UpdateAvailable { latest: String },
    /// No install receipt — the binary is managed by a package manager.
    DeferToPackageManager,
}

/// GitHub coordinates for the release source. `repograph-core`'s
/// `git_release_enable = false` guarantees the repo's latest GitHub Release is
/// the `repograph` binary tag, so this owner/repo resolves to the binary.
pub const GITHUB_OWNER: &str = "maikbasel";
pub const GITHUB_REPO: &str = "repograph";
pub const APP_NAME: &str = "repograph";

/// Opt-out environment variables for the passive notifier. The first is the
/// repograph-specific knob; the second is the cross-tool convention.
pub const NO_UPDATE_CHECK_ENV: &str = "REPOGRAPH_NO_UPDATE_CHECK";
pub const NO_UPDATE_NOTIFIER_ENV: &str = "NO_UPDATE_NOTIFIER";

/// Throttle window for the passive version check: contact the network at most
/// once per this many seconds.
pub const CACHE_TTL_SECS: i64 = 24 * 60 * 60;

/// Disposable on-disk cache that throttles the notifier's network checks. Never
/// authoritative — a missing or malformed file is simply a cache miss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheckCache {
    /// Unix timestamp (UTC seconds) of the last successful network check.
    pub last_checked_unix: i64,
    /// The latest version string seen at that check.
    pub latest_seen: String,
}

/// True iff `latest` is a strictly greater semver than `current`. Unparseable
/// input yields `false` — we never nag on garbage versions.
#[must_use]
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (Version::parse(current), Version::parse(latest)) {
        (Ok(current), Ok(latest)) => latest > current,
        _ => false,
    }
}

/// Pure gating decision for the passive notifier. The notice may be emitted
/// only when stdout is a TTY, the dispatched command was not `update`, and
/// neither opt-out variable is set.
// The four flags are independent gates checked at one call site; threading them
// through a struct would add ceremony without clarifying the boolean AND below.
#[allow(clippy::fn_params_excessive_bools)]
#[must_use]
pub const fn should_notify(
    stdout_is_tty: bool,
    command_is_update: bool,
    no_update_check_set: bool,
    no_notifier_set: bool,
) -> bool {
    stdout_is_tty && !command_is_update && !no_update_check_set && !no_notifier_set
}

/// True iff a check at `last_checked_unix` is still within `ttl_secs` of `now_unix`.
/// A future or equal timestamp counts as fresh; a malformed/negative delta is stale.
#[must_use]
pub const fn cache_is_fresh(last_checked_unix: i64, now_unix: i64, ttl_secs: i64) -> bool {
    now_unix.saturating_sub(last_checked_unix) < ttl_secs
}

/// Read the throttle cache. Returns `None` for a missing or malformed file —
/// the cache is disposable, so any read problem is treated as a miss.
#[must_use]
pub fn read_cache(path: &Path) -> Option<UpdateCheckCache> {
    // Disposable cache: a missing or unparseable file is a legitimate miss, not
    // a failure to surface — callers re-check the network on `None`.
    let raw = fs_err::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write the throttle cache, creating the parent directory if needed.
///
/// # Errors
/// Returns [`RepographError::Io`] if the directory or file cannot be written,
/// or [`RepographError::UpdateFailed`] if serialization fails.
pub fn write_cache(path: &Path, cache: &UpdateCheckCache) -> Result<(), RepographError> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(cache)
        .map_err(|e| RepographError::UpdateFailed(format!("cache serialize: {e}")))?;
    fs_err::write(path, json)?;
    Ok(())
}

/// Path of the throttle cache under the platform cache dir
/// (`~/.cache/repograph/update-check.json` on Linux).
#[must_use]
pub fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join(APP_NAME).join("update-check.json"))
}

/// Build the GitHub release source for `maikbasel/repograph`.
fn release_source() -> ReleaseSource {
    ReleaseSource {
        release_type: ReleaseSourceType::GitHub,
        owner: GITHUB_OWNER.to_string(),
        name: GITHUB_REPO.to_string(),
        app_name: APP_NAME.to_string(),
    }
}

/// Build a current-thread tokio runtime with the IO + time drivers reqwest
/// needs. Confining the runtime here keeps the rest of the CLI synchronous.
fn runtime() -> Result<tokio::runtime::Runtime, RepographError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| RepographError::UpdateFailed(format!("could not start async runtime: {e}")))
}

/// Query the latest available version from GitHub Releases, comparing against
/// the running version. Returns the latest version when one is available, or
/// `None` when the running build is current.
///
/// # Errors
/// Returns [`RepographError::UpdateFailed`] on a network, runtime, or version
/// parse failure.
pub fn query_latest() -> Result<Option<Version>, RepographError> {
    let mut updater = AxoUpdater::new_for(APP_NAME);
    updater.set_release_source(release_source());
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| RepographError::UpdateFailed(format!("bad current version: {e}")))?;
    updater
        .set_current_version(current)
        .map_err(|e| RepographError::UpdateFailed(e.to_string()))?;

    let rt = runtime()?;
    let latest = rt
        .block_on(updater.query_new_version())
        .map_err(|e| RepographError::UpdateFailed(e.to_string()))?
        .cloned();
    Ok(latest)
}

/// Run the update flow: load the install receipt, then either report (`check_only`)
/// or perform an in-place upgrade. When no receipt is present the caller is told
/// to defer to its package manager.
///
/// # Errors
/// Returns [`RepographError::UpdateFailed`] on network/IO/verification failures,
/// or [`RepographError::PermissionDenied`] when the binary cannot be replaced.
pub fn run_update(check_only: bool) -> Result<UpdateOutcome, RepographError> {
    let mut updater = AxoUpdater::new_for(APP_NAME);
    match updater.load_receipt() {
        Ok(_) => {}
        Err(AxoupdateError::NoReceipt { .. }) => return Ok(UpdateOutcome::DeferToPackageManager),
        Err(e) => return Err(RepographError::UpdateFailed(e.to_string())),
    }

    let rt = runtime()?;

    if check_only {
        let latest = rt
            .block_on(updater.query_new_version())
            .map_err(|e| RepographError::UpdateFailed(e.to_string()))?
            .cloned();
        return Ok(latest.map_or(UpdateOutcome::AlreadyCurrent, |version| {
            UpdateOutcome::UpdateAvailable {
                latest: version.to_string(),
            }
        }));
    }

    let needed = rt
        .block_on(updater.is_update_needed())
        .map_err(|e| RepographError::UpdateFailed(e.to_string()))?;
    if !needed {
        return Ok(UpdateOutcome::AlreadyCurrent);
    }

    // Resolve the install location up front so a permission failure can name it.
    let install_path = updater
        .install_prefix_root()
        .ok()
        .map(|p| p.as_std_path().to_path_buf());

    match rt.block_on(updater.run()) {
        Ok(Some(result)) => Ok(UpdateOutcome::Updated {
            from: result.old_version.map(|v| v.to_string()),
            to: result.new_version.to_string(),
        }),
        Ok(None) => Ok(UpdateOutcome::AlreadyCurrent),
        Err(e) => Err(classify_run_error(&e, install_path)),
    }
}

/// Map an `axoupdater` run failure to the right `RepographError`: a write
/// permission failure becomes [`RepographError::PermissionDenied`] (exit `4`),
/// everything else becomes [`RepographError::UpdateFailed`] (exit `1`).
fn classify_run_error(err: &AxoupdateError, install_path: Option<PathBuf>) -> RepographError {
    if has_permission_denied(err) {
        RepographError::PermissionDenied {
            path: install_path.unwrap_or_else(|| PathBuf::from("the repograph binary")),
        }
    } else {
        RepographError::UpdateFailed(err.to_string())
    }
}

/// Walk an error's `source` chain looking for an [`std::io::Error`] with kind
/// [`ErrorKind::PermissionDenied`].
fn has_permission_denied(err: &(dyn Error + 'static)) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(err);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            if io.kind() == ErrorKind::PermissionDenied {
                return true;
            }
        }
        current = e.source();
    }
    false
}

/// True when an environment variable is present and non-empty.
fn env_flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Resolve the latest version through the throttle cache: return the cached
/// value while it is fresh, otherwise hit the network once and rewrite the
/// cache. Fail-silent — any error yields `None`. An up-to-date result is cached
/// as the running version so the *next* run is also throttled.
fn latest_throttled() -> Option<String> {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let path = cache_path();

    if let Some(path) = path.as_ref() {
        if let Some(cache) = read_cache(path) {
            if cache_is_fresh(cache.last_checked_unix, now, CACHE_TTL_SECS) {
                return Some(cache.latest_seen);
            }
        }
    }

    let latest = match query_latest() {
        Ok(Some(version)) => version.to_string(),
        // Up to date: record the running version so we still throttle for the TTL.
        Ok(None) => env!("CARGO_PKG_VERSION").to_string(),
        // Network/runtime failure: stay silent and don't cache, so we retry next run.
        Err(_) => return None,
    };

    if let Some(path) = path {
        let _ = write_cache(
            &path,
            &UpdateCheckCache {
                last_checked_unix: now,
                latest_seen: latest.clone(),
            },
        );
    }
    Some(latest)
}

/// Post-command passive notifier. Emits at most one stderr line nudging the user
/// to upgrade. Fail-silent by contract: any gating miss, network error, or write
/// failure produces no output and never changes the process exit code.
pub fn notify(command_is_update: bool) {
    use is_terminal::IsTerminal;

    let stdout_is_tty = std::io::stdout().is_terminal();
    if !should_notify(
        stdout_is_tty,
        command_is_update,
        env_flag(NO_UPDATE_CHECK_ENV),
        env_flag(NO_UPDATE_NOTIFIER_ENV),
    ) {
        return;
    }

    let current = env!("CARGO_PKG_VERSION");
    let Some(latest) = latest_throttled() else {
        return;
    };
    if is_newer(current, &latest) {
        // Best-effort stderr write; a failure here is not worth surfacing.
        let mut stderr = std::io::stderr().lock();
        let _ = crate::output::render_update_notice(&mut stderr, current, &latest);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn is_newer_true_when_latest_greater() {
        assert!(is_newer("0.2.1", "0.3.0"));
        assert!(is_newer("0.2.1", "0.2.2"));
        assert!(is_newer("1.0.0", "2.0.0"));
    }

    #[test]
    fn is_newer_false_when_equal_or_older() {
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.3.0", "0.2.9"));
        assert!(!is_newer("2.0.0", "1.9.9"));
    }

    #[test]
    fn is_newer_false_on_unparseable() {
        assert!(!is_newer("not-a-version", "0.3.0"));
        assert!(!is_newer("0.2.1", "garbage"));
    }

    #[test]
    fn is_newer_respects_prerelease_ordering() {
        // A pre-release is older than its release per semver.
        assert!(is_newer("0.3.0-rc.1", "0.3.0"));
        assert!(!is_newer("0.3.0", "0.3.0-rc.1"));
    }

    #[test]
    fn should_notify_true_only_when_all_gates_pass() {
        assert!(should_notify(true, false, false, false));
    }

    #[test]
    fn should_notify_false_when_stdout_not_tty() {
        assert!(!should_notify(false, false, false, false));
    }

    #[test]
    fn should_notify_false_for_update_command() {
        assert!(!should_notify(true, true, false, false));
    }

    #[test]
    fn should_notify_false_when_opted_out() {
        assert!(!should_notify(true, false, true, false));
        assert!(!should_notify(true, false, false, true));
    }

    #[test]
    fn cache_fresh_within_ttl() {
        assert!(cache_is_fresh(1_000, 1_000 + CACHE_TTL_SECS - 1, CACHE_TTL_SECS));
        assert!(cache_is_fresh(1_000, 1_000, CACHE_TTL_SECS)); // same instant
    }

    #[test]
    fn cache_stale_at_or_past_ttl() {
        assert!(!cache_is_fresh(1_000, 1_000 + CACHE_TTL_SECS, CACHE_TTL_SECS));
        assert!(!cache_is_fresh(1_000, 1_000 + CACHE_TTL_SECS + 1, CACHE_TTL_SECS));
    }

    #[test]
    fn cache_roundtrips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("update-check.json");
        let cache = UpdateCheckCache {
            last_checked_unix: 1_700_000_000,
            latest_seen: "0.3.0".to_string(),
        };
        write_cache(&path, &cache).unwrap();
        assert_eq!(read_cache(&path), Some(cache));
    }

    #[test]
    fn read_cache_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_cache(&dir.path().join("absent.json")), None);
    }

    #[test]
    fn read_cache_malformed_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"{ not valid json").unwrap();
        assert_eq!(read_cache(&path), None);
    }

    #[derive(Debug)]
    struct Wrapper(std::io::Error);
    impl std::fmt::Display for Wrapper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "wrapper")
        }
    }
    impl Error for Wrapper {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn permission_denied_found_through_source_chain() {
        let err = Wrapper(std::io::Error::from(ErrorKind::PermissionDenied));
        assert!(has_permission_denied(&err));
    }

    #[test]
    fn permission_denied_at_top_level() {
        let err = std::io::Error::from(ErrorKind::PermissionDenied);
        assert!(has_permission_denied(&err));
    }

    #[test]
    fn non_permission_error_is_not_flagged() {
        let err = Wrapper(std::io::Error::from(ErrorKind::NotFound));
        assert!(!has_permission_denied(&err));
    }
}
