//! Config model and TOML persistence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::RepographError;

/// On-disk file name within the config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// A registered local git repository. The `name` is the map key in
/// [`Config::repos`] — it does not appear as a field on this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<String>,
}

/// Top-level config aggregating all registered repos. Workspaces will land
/// alongside in `workspace-support` (Phase 3).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, rename = "repo", skip_serializing_if = "BTreeMap::is_empty")]
    repos: BTreeMap<String, Repo>,
}

impl Config {
    /// Read-only view of the registered repos.
    #[must_use]
    pub const fn repos(&self) -> &BTreeMap<String, Repo> {
        &self.repos
    }

    /// Platform-default config directory: `dirs::config_dir() / "repograph"`.
    /// Returns `None` when no platform default exists (e.g. minimal envs); the
    /// binary surfaces this as a usage error guiding the user to `--config-dir`.
    #[must_use]
    pub fn default_dir() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("repograph"))
    }

    /// Load config from `dir/config.toml`. Missing file → empty `Config`.
    /// Malformed TOML → `RepographError::ConfigParse`.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Io`] for filesystem failures, or
    /// [`RepographError::ConfigParse`] when the file exists but is not valid TOML.
    pub fn load(dir: &Path) -> Result<Self, RepographError> {
        let path = dir.join(CONFIG_FILE_NAME);
        match fs_err::read_to_string(&path) {
            Ok(body) => Ok(toml::from_str(&body)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Save config atomically to `dir/config.toml`, creating `dir` if missing.
    ///
    /// Atomicity: we serialize to a sibling temp file, then `rename` to the
    /// target. A crash mid-write cannot leave the target half-written.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::ConfigWrite`] when serialization fails,
    /// [`RepographError::PermissionDenied`] when the target dir or file is not
    /// writable, or [`RepographError::Io`] for other filesystem failures.
    pub fn save(&self, dir: &Path) -> Result<(), RepographError> {
        let body = toml::to_string_pretty(self)?;
        let target = dir.join(CONFIG_FILE_NAME);

        if let Err(e) = fs_err::create_dir_all(dir) {
            return Err(map_io_to_perm(e, dir));
        }

        let tmp = dir.join(format!(".{CONFIG_FILE_NAME}.tmp"));
        if let Err(e) = fs_err::write(&tmp, body.as_bytes()) {
            return Err(map_io_to_perm(e, &tmp));
        }
        if let Err(e) = fs_err::rename(&tmp, &target) {
            return Err(map_io_to_perm(e, &target));
        }
        Ok(())
    }

    /// Register a repo, enforcing both name and path uniqueness.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::Conflict`] with `kind = "name"` when `name`
    /// is already registered, or `kind = "path"` when `repo.path` is already
    /// registered under a different name.
    pub fn add_repo(&mut self, name: String, repo: Repo) -> Result<(), RepographError> {
        if self.repos.contains_key(&name) {
            return Err(RepographError::Conflict { kind: "name", name });
        }
        if let Some((existing_name, _)) = self.repos.iter().find(|(_, r)| r.path == repo.path) {
            return Err(RepographError::Conflict {
                kind: "path",
                name: existing_name.clone(),
            });
        }
        self.repos.insert(name, repo);
        Ok(())
    }

    /// Deregister a repo by name.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::NotFound`] when no repo by that name is registered.
    pub fn remove_repo(&mut self, name: &str) -> Result<Repo, RepographError> {
        self.repos
            .remove(name)
            .ok_or_else(|| RepographError::NotFound {
                kind: "repo",
                name: name.to_string(),
            })
    }
}

/// Map an [`std::io::Error`] to a typed permission-denied error when the kind
/// matches; otherwise pass it through as `Io`.
fn map_io_to_perm(e: std::io::Error, path: &Path) -> RepographError {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        RepographError::PermissionDenied {
            path: path.to_path_buf(),
        }
    } else {
        RepographError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::TempDir;

    fn make(path: &str) -> Repo {
        Repo {
            path: PathBuf::from(path),
            description: None,
            stack: vec![],
        }
    }

    #[test]
    fn load_missing_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let cfg = Config::load(tmp.path()).unwrap();
        assert!(cfg.repos.is_empty());
    }

    #[test]
    fn save_then_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.add_repo("foo".into(), make("/tmp/foo")).unwrap();
        cfg.add_repo(
            "bar".into(),
            Repo {
                path: PathBuf::from("/tmp/bar"),
                description: Some("hi".into()),
                stack: vec!["rust".into()],
            },
        )
        .unwrap();

        cfg.save(tmp.path()).unwrap();
        let loaded = Config::load(tmp.path()).unwrap();
        assert_eq!(loaded.repos.len(), 2);
        assert_eq!(
            loaded.repos.get("bar").unwrap().description.as_deref(),
            Some("hi")
        );
    }

    #[test]
    fn name_conflict_blocks_insert() {
        let mut cfg = Config::default();
        cfg.add_repo("foo".into(), make("/a")).unwrap();
        let err = cfg.add_repo("foo".into(), make("/b")).unwrap_err();
        assert!(matches!(err, RepographError::Conflict { kind: "name", .. }));
        assert_eq!(cfg.repos.get("foo").unwrap().path, PathBuf::from("/a"));
    }

    #[test]
    fn path_conflict_blocks_insert() {
        let mut cfg = Config::default();
        cfg.add_repo("foo".into(), make("/shared")).unwrap();
        let err = cfg.add_repo("bar".into(), make("/shared")).unwrap_err();
        assert!(matches!(err, RepographError::Conflict { kind: "path", .. }));
        assert!(!cfg.repos.contains_key("bar"));
    }

    #[test]
    fn remove_missing_returns_not_found() {
        let mut cfg = Config::default();
        let err = cfg.remove_repo("ghost").unwrap_err();
        assert!(matches!(err, RepographError::NotFound { .. }));
    }

    #[test]
    fn unknown_field_in_toml_is_tolerated() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(CONFIG_FILE_NAME),
            "[repo.foo]\npath = \"/tmp/foo\"\nfuture = \"yes\"\n",
        )
        .unwrap();
        let cfg = Config::load(tmp.path()).unwrap();
        assert!(cfg.repos.contains_key("foo"));
    }
}
