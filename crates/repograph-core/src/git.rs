//! `git2`-backed introspection helpers.

use std::path::{Path, PathBuf};

use crate::error::RepographError;

/// Verify that `path` is a git repository, returning its canonical absolute
/// form on success. Symlinks are resolved; relative inputs are absolutized.
///
/// # Errors
///
/// Returns [`RepographError::NotFound`] when `path` does not exist on disk, or
/// [`RepographError::GitOpen`] when it exists but is not a git repository.
pub fn validate_git_repo(path: &Path) -> Result<PathBuf, RepographError> {
    let canonical = match fs_err::canonicalize(path) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RepographError::NotFound {
                kind: "path",
                name: path.display().to_string(),
            });
        }
        Err(e) => return Err(e.into()),
    };

    git2::Repository::open(&canonical).map_err(|source| RepographError::GitOpen {
        path: canonical.clone(),
        source,
    })?;

    Ok(canonical)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rejects_nonexistent_path() {
        let tmp = TempDir::new().unwrap();
        let err = validate_git_repo(&tmp.path().join("nope")).unwrap_err();
        assert!(matches!(err, RepographError::NotFound { kind: "path", .. }));
    }

    #[test]
    fn rejects_non_git_directory() {
        let tmp = TempDir::new().unwrap();
        let err = validate_git_repo(tmp.path()).unwrap_err();
        assert!(matches!(err, RepographError::GitOpen { .. }));
    }

    #[test]
    fn accepts_real_git_repo_returns_canonical() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("r");
        std::fs::create_dir_all(&repo_path).unwrap();
        git2::Repository::init(&repo_path).unwrap();

        let resolved = validate_git_repo(&repo_path).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&repo_path).unwrap());
    }
}
