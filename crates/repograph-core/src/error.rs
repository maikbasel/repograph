//! Error type for the repograph domain.
//!
//! Every variant maps to a documented exit code via `exit_code()`. The code map
//! is the binary's contract with downstream consumers (humans, agents, CI).

use std::path::PathBuf;

/// Domain error for the repograph core and binary. Each variant is wired to a
/// specific exit code documented in `CLAUDE.md` and the `registry-core` spec.
#[derive(Debug, thiserror::Error)]
pub enum RepographError {
    /// Generic I/O failure; permission-denied is detected and mapped to code 4.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// Config file present but not parseable as TOML.
    #[error("invalid TOML in config file: {0}")]
    ConfigParse(#[from] toml::de::Error),

    /// Could not serialize config to TOML.
    #[error("failed to serialize config: {0}")]
    ConfigWrite(#[from] toml::ser::Error),

    /// `git2::Repository::open` rejected the path.
    #[error("not a git repository: {path}: {source}")]
    GitOpen {
        path: PathBuf,
        #[source]
        source: git2::Error,
    },

    /// A required entity does not exist (repo, workspace, path).
    #[error("{kind} '{name}' not found")]
    NotFound { kind: &'static str, name: String },

    /// A unique-constraint violation (name or path already registered).
    #[error("{kind} '{name}' already registered")]
    Conflict { kind: &'static str, name: String },

    /// Explicit permission failure (raised when we can attribute it to a known path).
    #[error("permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    /// Runtime usage failure (e.g. no config-dir resolvable). CLI argument
    /// errors are handled by clap and exit with code 2 directly.
    #[error("{0}")]
    UsageError(String),

    /// User-supplied identifier violates a naming rule (e.g. workspace name
    /// fails the RFC 1123 label policy). Maps to exit code `2`, matching how
    /// clap reports bad arguments.
    #[error("invalid {kind} name '{name}': {reason}")]
    InvalidName {
        kind: &'static str,
        name: String,
        reason: &'static str,
    },

    /// An interactive code path required a TTY but stdout was redirected, and
    /// no non-interactive escape hatch (flag, env var) was provided. Maps to
    /// exit code `2`. The payload is the full user-visible guidance message
    /// (e.g. "agents not configured; run `repograph init`" or "stdout is not
    /// a TTY; pass `--no-prompt --agents <list>` …").
    #[error("{0}")]
    NeedsInit(String),

    /// `repograph doctor` found one or more error-severity findings. The
    /// report is the success output (already written to stdout); this variant
    /// only carries the exit-code signal. Maps to exit code `1`. The binary
    /// special-cases this variant to suppress the generic "repograph failed"
    /// `tracing::error!` line, since the report itself is the user-facing
    /// surface, not the error message.
    #[error("doctor found {count} error finding(s) — see report above")]
    DoctorErrorsFound { count: u32 },

    /// `repograph update` failed to reach, download, or verify a release.
    /// Covers network/IO failures and checksum/signature verification
    /// failures. Maps to exit code `1`. A binary-write permission failure is
    /// reported through [`RepographError::PermissionDenied`] (exit `4`)
    /// instead, so this variant is reserved for general update failures.
    #[error("update failed: {0}")]
    UpdateFailed(String),

    /// `repograph find` was invoked before any search index was built. The
    /// index is a "resource" that does not exist yet, so this maps to exit
    /// code `3` (not-found), mirroring a missing repo/workspace. The Display
    /// text guides the user to `repograph index`.
    #[error("no search index found — run `repograph index` first")]
    IndexMissing,

    /// The search index database is present but could not be opened, read, or
    /// queried (corruption, a schema the binary can't drive, a failed SQL
    /// statement). Maps to exit code `1`. A missing index is
    /// [`RepographError::IndexMissing`] (exit `3`) instead.
    #[error("search index error: {0}")]
    Index(String),
}

impl From<rusqlite::Error> for RepographError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Index(e.to_string())
    }
}

impl RepographError {
    /// Map this error to the documented exit code:
    /// `1` general, `3` not-found, `4` permission-denied, `5` conflict.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => 4,
            Self::PermissionDenied { .. } => 4,
            Self::GitOpen { .. } | Self::NotFound { .. } | Self::IndexMissing => 3,
            Self::Conflict { .. } => 5,
            Self::InvalidName { .. } | Self::NeedsInit { .. } | Self::UsageError(_) => 2,
            Self::Io(_)
            | Self::ConfigParse(_)
            | Self::ConfigWrite(_)
            | Self::DoctorErrorsFound { .. }
            | Self::UpdateFailed(_)
            | Self::Index(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn io_permission_denied_maps_to_4() {
        let err = RepographError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn other_io_maps_to_1() {
        let err = RepographError::Io(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn explicit_permission_denied_maps_to_4() {
        let err = RepographError::PermissionDenied {
            path: PathBuf::from("/nope"),
        };
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn not_found_maps_to_3() {
        let err = RepographError::NotFound {
            kind: "repo",
            name: "foo".into(),
        };
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn git_open_maps_to_3() {
        let err = RepographError::GitOpen {
            path: PathBuf::from("/tmp/x"),
            source: git2::Error::from_str("synthetic"),
        };
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn conflict_maps_to_5() {
        let err = RepographError::Conflict {
            kind: "name",
            name: "foo".into(),
        };
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn usage_error_maps_to_2() {
        let err = RepographError::UsageError("nope".into());
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn invalid_name_maps_to_2() {
        let err = RepographError::InvalidName {
            kind: "workspace",
            name: "Bad Name".into(),
            reason: "must be lowercase",
        };
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn needs_init_maps_to_2() {
        let err = RepographError::NeedsInit("agents not configured; run `repograph init`".into());
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("repograph init"));
    }

    #[test]
    fn update_failed_maps_to_1() {
        let err = RepographError::UpdateFailed("network unreachable".into());
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn index_missing_maps_to_3_and_names_index_command() {
        let err = RepographError::IndexMissing;
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains("repograph index"));
    }

    #[test]
    fn index_error_maps_to_1() {
        let err = RepographError::Index("disk image is malformed".into());
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn config_parse_maps_to_1() {
        let err: RepographError = toml::from_str::<toml::Value>("[unterminated")
            .unwrap_err()
            .into();
        assert_eq!(err.exit_code(), 1);
    }
}
