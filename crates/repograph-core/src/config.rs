//! Config model and TOML persistence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::RepographError;

/// On-disk file name within the config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Maximum length of a workspace name (RFC 1123 label rule).
pub const MAX_WORKSPACE_NAME_LEN: usize = 63;

/// Reserved workspace names. These collide with future filter ergonomics
/// (e.g. `--workspace all`) and are rejected at write time.
pub const RESERVED_WORKSPACE_NAMES: &[&str] = &["default", "all", "none"];

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

/// A named grouping of registered repositories.
///
/// The `name` is the map key in [`Config::workspaces`] — it does not appear
/// as a field on this struct. `members` holds bare repo names (keys into
/// [`Config::repos`]) and is kept sorted on write for round-trip stability.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub members: Vec<String>,
}

/// Result of resolving a workspace's `members` against the repo registry:
/// `(live, dangling)`. Live entries borrow the repo's name and its
/// [`Repo`] entry; dangling entries borrow only the orphaned name.
pub type WorkspaceResolution<'a> = (Vec<(&'a String, &'a Repo)>, Vec<&'a String>);

/// Top-level config aggregating all registered repos and workspaces.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, rename = "repo", skip_serializing_if = "BTreeMap::is_empty")]
    repos: BTreeMap<String, Repo>,
    #[serde(
        default,
        rename = "workspace",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    workspaces: BTreeMap<String, Workspace>,
}

impl Config {
    /// Read-only view of the registered repos.
    #[must_use]
    pub const fn repos(&self) -> &BTreeMap<String, Repo> {
        &self.repos
    }

    /// Read-only view of the registered workspaces.
    #[must_use]
    pub const fn workspaces(&self) -> &BTreeMap<String, Workspace> {
        &self.workspaces
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

    /// Create an empty workspace under `name` with an optional description.
    ///
    /// The name must satisfy [`validate_workspace_name`]. The workspace must
    /// not already exist.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::InvalidName`] when `name` violates the
    /// naming policy, or [`RepographError::Conflict`] with `kind = "workspace"`
    /// when a workspace by that name already exists.
    pub fn create_workspace(
        &mut self,
        name: String,
        description: Option<String>,
    ) -> Result<(), RepographError> {
        validate_workspace_name(&name)?;
        if self.workspaces.contains_key(&name) {
            return Err(RepographError::Conflict {
                kind: "workspace",
                name,
            });
        }
        self.workspaces.insert(
            name,
            Workspace {
                description: description.filter(|s| !s.is_empty()),
                members: Vec::new(),
            },
        );
        Ok(())
    }

    /// Delete a workspace by name. Registered repos are untouched.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::NotFound`] with `kind = "workspace"` when
    /// no workspace by that name is registered.
    pub fn remove_workspace(&mut self, name: &str) -> Result<Workspace, RepographError> {
        self.workspaces
            .remove(name)
            .ok_or_else(|| RepographError::NotFound {
                kind: "workspace",
                name: name.to_string(),
            })
    }

    /// Atomically attach one or more registered repos to a workspace.
    ///
    /// All `repos` must be registered before any mutation occurs; if even one
    /// is missing, the workspace is left unchanged. Already-member repos are
    /// silently ignored. On success the `members` list is sorted and
    /// deduplicated.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::NotFound`] with `kind = "workspace"` when
    /// the workspace does not exist, or `kind = "repo"` (naming the first
    /// missing repo) when any input repo is not registered.
    pub fn add_members(&mut self, workspace: &str, repos: &[String]) -> Result<(), RepographError> {
        // Workspace presence first, so the error message names the right thing
        // when neither workspace nor any of the repos exists.
        if !self.workspaces.contains_key(workspace) {
            return Err(RepographError::NotFound {
                kind: "workspace",
                name: workspace.to_string(),
            });
        }
        for name in repos {
            if !self.repos.contains_key(name) {
                return Err(RepographError::NotFound {
                    kind: "repo",
                    name: name.clone(),
                });
            }
        }
        // Re-fetch as mutable; we re-emit NotFound rather than expect() so a
        // future refactor that drops the contains_key guard can't introduce a panic.
        let ws = self
            .workspaces
            .get_mut(workspace)
            .ok_or_else(|| RepographError::NotFound {
                kind: "workspace",
                name: workspace.to_string(),
            })?;
        for name in repos {
            ws.members.push(name.clone());
        }
        ws.members.sort();
        ws.members.dedup();
        Ok(())
    }

    /// Detach one or more repos from a workspace. Non-members are silently
    /// ignored. The repo registry is not modified.
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::NotFound`] with `kind = "workspace"` when
    /// the workspace does not exist.
    pub fn remove_members(
        &mut self,
        workspace: &str,
        repos: &[String],
    ) -> Result<(), RepographError> {
        let ws = self
            .workspaces
            .get_mut(workspace)
            .ok_or_else(|| RepographError::NotFound {
                kind: "workspace",
                name: workspace.to_string(),
            })?;
        ws.members.retain(|m| !repos.iter().any(|r| r == m));
        Ok(())
    }

    /// Walk a workspace's members and partition them into live entries
    /// (resolved against the repo registry) and dangling names (tombstoned
    /// references to repos that are no longer registered). The order in each
    /// returned vector matches the workspace's stored `members` order
    /// (alphabetical after sort-on-write).
    ///
    /// # Errors
    ///
    /// Returns [`RepographError::NotFound`] with `kind = "workspace"` when
    /// the workspace does not exist.
    pub fn resolve_workspace<'a>(
        &'a self,
        workspace: &str,
    ) -> Result<WorkspaceResolution<'a>, RepographError> {
        let ws = self
            .workspaces
            .get(workspace)
            .ok_or_else(|| RepographError::NotFound {
                kind: "workspace",
                name: workspace.to_string(),
            })?;
        let mut live = Vec::with_capacity(ws.members.len());
        let mut dangling = Vec::new();
        for name in &ws.members {
            if let Some((key, repo)) = self.repos.get_key_value(name) {
                live.push((key, repo));
            } else {
                dangling.push(name);
            }
        }
        Ok((live, dangling))
    }
}

/// Enforce the workspace naming policy: lowercase ASCII alphanumerics and
/// hyphens, must start alphanumeric, length 1..=63, and not one of the
/// reserved words.
///
/// # Errors
///
/// Returns [`RepographError::InvalidName`] with `kind = "workspace"` when the
/// name violates the policy. The `reason` text is a short, user-facing phrase.
pub fn validate_workspace_name(name: &str) -> Result<(), RepographError> {
    if name.is_empty() {
        return Err(invalid_workspace_name(name, "must not be empty"));
    }
    if name.len() > MAX_WORKSPACE_NAME_LEN {
        return Err(invalid_workspace_name(name, "must be at most 63 characters"));
    }
    if RESERVED_WORKSPACE_NAMES.contains(&name) {
        return Err(invalid_workspace_name(name, "is a reserved name"));
    }
    for (i, c) in name.chars().enumerate() {
        let alnum_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        if i == 0 {
            if !alnum_lower {
                return Err(invalid_workspace_name(
                    name,
                    "must start with a lowercase letter or digit",
                ));
            }
        } else if !alnum_lower && c != '-' {
            return Err(invalid_workspace_name(
                name,
                "must contain only lowercase letters, digits, and hyphens",
            ));
        }
    }
    Ok(())
}

fn invalid_workspace_name(name: &str, reason: &'static str) -> RepographError {
    RepographError::InvalidName {
        kind: "workspace",
        name: name.to_string(),
        reason,
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

    // --- Workspace tests ---

    #[test]
    fn validate_workspace_name_accepts_simple_lowercase() {
        assert!(validate_workspace_name("acme").is_ok());
        assert!(validate_workspace_name("acme-rebuild-2026").is_ok());
        assert!(validate_workspace_name("a").is_ok());
        assert!(validate_workspace_name("0").is_ok());
        assert!(validate_workspace_name("0acme").is_ok());
    }

    #[test]
    fn validate_workspace_name_rejects_empty() {
        let err = validate_workspace_name("").unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn validate_workspace_name_rejects_uppercase() {
        let err = validate_workspace_name("AcmeRebuild").unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
    }

    #[test]
    fn validate_workspace_name_rejects_leading_hyphen() {
        let err = validate_workspace_name("-acme").unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
    }

    #[test]
    fn validate_workspace_name_rejects_underscore() {
        let err = validate_workspace_name("ac_me").unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
    }

    #[test]
    fn validate_workspace_name_rejects_spaces() {
        let err = validate_workspace_name("ac me").unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
    }

    #[test]
    fn validate_workspace_name_rejects_overlength() {
        let name = "a".repeat(MAX_WORKSPACE_NAME_LEN + 1);
        let err = validate_workspace_name(&name).unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
    }

    #[test]
    fn validate_workspace_name_accepts_exact_max_length() {
        let name = "a".repeat(MAX_WORKSPACE_NAME_LEN);
        assert!(validate_workspace_name(&name).is_ok());
    }

    #[test]
    fn validate_workspace_name_rejects_reserved_words() {
        for reserved in RESERVED_WORKSPACE_NAMES {
            let err = validate_workspace_name(reserved).unwrap_err();
            assert!(
                matches!(err, RepographError::InvalidName { .. }),
                "reserved `{reserved}` must be rejected"
            );
        }
    }

    #[test]
    fn create_workspace_inserts_empty_entry() {
        let mut cfg = Config::default();
        cfg.create_workspace("acme".into(), None).unwrap();
        let ws = cfg.workspaces.get("acme").unwrap();
        assert!(ws.description.is_none());
        assert!(ws.members.is_empty());
    }

    #[test]
    fn create_workspace_persists_description() {
        let mut cfg = Config::default();
        cfg.create_workspace("acme".into(), Some("rebuild".into()))
            .unwrap();
        assert_eq!(
            cfg.workspaces.get("acme").unwrap().description.as_deref(),
            Some("rebuild")
        );
    }

    #[test]
    fn create_workspace_conflict_returns_conflict() {
        let mut cfg = Config::default();
        cfg.create_workspace("acme".into(), None).unwrap();
        let err = cfg.create_workspace("acme".into(), None).unwrap_err();
        assert!(matches!(
            err,
            RepographError::Conflict {
                kind: "workspace",
                ..
            }
        ));
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn create_workspace_invalid_name_returns_invalid_name() {
        let mut cfg = Config::default();
        let err = cfg
            .create_workspace("Bad Name".into(), None)
            .unwrap_err();
        assert!(matches!(err, RepographError::InvalidName { .. }));
        assert_eq!(err.exit_code(), 2);
        assert!(cfg.workspaces.is_empty());
    }

    #[test]
    fn remove_workspace_missing_returns_not_found() {
        let mut cfg = Config::default();
        let err = cfg.remove_workspace("ghost").unwrap_err();
        assert!(matches!(
            err,
            RepographError::NotFound {
                kind: "workspace",
                ..
            }
        ));
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn remove_workspace_does_not_touch_repos() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["api".into()]).unwrap();
        cfg.remove_workspace("acme").unwrap();
        assert!(cfg.repos.contains_key("api"));
        assert!(!cfg.workspaces.contains_key("acme"));
    }

    #[test]
    fn add_members_atomic_when_one_repo_missing() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.add_repo("ui".into(), make("/tmp/ui")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        let err = cfg
            .add_members("acme", &["api".into(), "ghost".into(), "ui".into()])
            .unwrap_err();
        assert!(matches!(
            err,
            RepographError::NotFound { kind: "repo", ref name } if name == "ghost"
        ));
        // No partial application.
        assert!(cfg.workspaces.get("acme").unwrap().members.is_empty());
    }

    #[test]
    fn add_members_sorts_and_deduplicates() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.add_repo("ui".into(), make("/tmp/ui")).unwrap();
        cfg.add_repo("libs".into(), make("/tmp/libs")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["ui".into(), "api".into(), "libs".into()])
            .unwrap();
        assert_eq!(
            cfg.workspaces.get("acme").unwrap().members,
            vec!["api", "libs", "ui"]
        );
        // Idempotent.
        cfg.add_members("acme", &["api".into()]).unwrap();
        assert_eq!(
            cfg.workspaces.get("acme").unwrap().members,
            vec!["api", "libs", "ui"]
        );
    }

    #[test]
    fn add_members_missing_workspace_returns_not_found() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        let err = cfg.add_members("ghost", &["api".into()]).unwrap_err();
        assert!(matches!(
            err,
            RepographError::NotFound {
                kind: "workspace",
                ..
            }
        ));
    }

    #[test]
    fn remove_members_is_idempotent_for_non_members() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["api".into()]).unwrap();
        cfg.remove_members("acme", &["ghost".into()]).unwrap();
        assert_eq!(cfg.workspaces.get("acme").unwrap().members, vec!["api"]);
    }

    #[test]
    fn remove_members_does_not_deregister_repo() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["api".into()]).unwrap();
        cfg.remove_members("acme", &["api".into()]).unwrap();
        assert!(cfg.repos.contains_key("api"));
        assert!(cfg.workspaces.get("acme").unwrap().members.is_empty());
    }

    #[test]
    fn resolve_workspace_partitions_live_and_dangling() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.add_repo("ui".into(), make("/tmp/ui")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["api".into(), "ui".into()])
            .unwrap();
        // Tombstone: forcibly drop `ui` from the registry without touching the workspace.
        cfg.remove_repo("ui").unwrap();
        let (live, dangling) = cfg.resolve_workspace("acme").unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].0, "api");
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0], "ui");
    }

    #[test]
    fn resolve_workspace_recovers_after_reregistration() {
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["api".into()]).unwrap();
        cfg.remove_repo("api").unwrap();
        let (_, dangling) = cfg.resolve_workspace("acme").unwrap();
        assert_eq!(dangling, vec!["api"]);
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        let (live, dangling) = cfg.resolve_workspace("acme").unwrap();
        assert_eq!(live.len(), 1);
        assert!(dangling.is_empty());
    }

    #[test]
    fn round_trip_with_mixed_repos_and_workspaces() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.add_repo("api".into(), make("/tmp/api")).unwrap();
        cfg.add_repo("ui".into(), make("/tmp/ui")).unwrap();
        cfg.create_workspace("acme".into(), Some("Rebuild".into()))
            .unwrap();
        cfg.add_members("acme", &["ui".into(), "api".into()])
            .unwrap();
        cfg.create_workspace("billing".into(), None).unwrap();
        cfg.save(tmp.path()).unwrap();

        let body_first = fs_err::read_to_string(tmp.path().join(CONFIG_FILE_NAME)).unwrap();
        let loaded = Config::load(tmp.path()).unwrap();
        loaded.save(tmp.path()).unwrap();
        let body_second = fs_err::read_to_string(tmp.path().join(CONFIG_FILE_NAME)).unwrap();
        assert_eq!(body_first, body_second, "byte-identical round trip");

        assert_eq!(loaded.workspaces.len(), 2);
        let acme = loaded.workspaces.get("acme").unwrap();
        assert_eq!(acme.description.as_deref(), Some("Rebuild"));
        assert_eq!(acme.members, vec!["api", "ui"]);
        let billing = loaded.workspaces.get("billing").unwrap();
        assert!(billing.description.is_none());
        assert!(billing.members.is_empty());
    }

    #[test]
    fn unknown_field_on_workspace_is_tolerated() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(CONFIG_FILE_NAME),
            "[workspace.acme]\nmembers = []\nfuture = \"yes\"\n",
        )
        .unwrap();
        let cfg = Config::load(tmp.path()).unwrap();
        assert!(cfg.workspaces.contains_key("acme"));
    }
}
