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
}

impl RepographError {
    /// Map this error to the documented exit code:
    /// `1` general, `3` not-found, `4` permission-denied, `5` conflict.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => 4,
            Self::PermissionDenied { .. } => 4,
            Self::GitOpen { .. } | Self::NotFound { .. } => 3,
            Self::Conflict { .. } => 5,
            Self::Io(_)
            | Self::ConfigParse(_)
            | Self::ConfigWrite(_)
            | Self::UsageError(_) => 1,
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
    fn usage_error_maps_to_1() {
        let err = RepographError::UsageError("nope".into());
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
