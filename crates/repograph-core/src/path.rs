//! Path canonicalization that yields shell-usable paths on every platform.
//!
//! Every registered repo path flows through [`canonicalize`] before it is
//! stored in the registry, inspected for status, or emitted by `switch`. The
//! result must therefore be a path the user's shell can `cd` into directly.

use std::path::{Path, PathBuf};

/// Canonicalize `path`, stripping the Windows `\\?\` verbatim prefix whenever
/// the simplified form is unambiguous. On Unix this is a plain canonicalize.
///
/// `std::fs::canonicalize` (and `fs_err`'s wrapper) return extended-length
/// `\\?\C:\…` paths on Windows. Those break every consumer that matters here:
/// `cd '\\?\C:\…'` fails in `cmd.exe`, the prefix surfaces verbatim in
/// `list` / `status` / `doctor` output, and the leading `\\?\` is not what a
/// user ever typed. [`dunce::simplified`] drops the prefix when the path stays
/// valid without it (the common case for repo paths) and preserves it
/// untouched when it is genuinely required — paths beyond `MAX_PATH`, or
/// components illegal in a normal Win32 path.
///
/// # Errors
///
/// Propagates the [`std::io::Error`] from the underlying canonicalize — most
/// commonly [`std::io::ErrorKind::NotFound`] when `path` does not exist —
/// carrying `fs_err`'s path context in the message.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = fs_err::canonicalize(path)?;
    Ok(dunce::simplified(&canonical).to_path_buf())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn canonicalizes_existing_dir_to_absolute() {
        let tmp = TempDir::new().unwrap();
        let resolved = canonicalize(tmp.path()).unwrap();
        assert!(resolved.is_absolute());
    }

    #[test]
    #[cfg(windows)]
    fn strips_verbatim_prefix_on_windows() {
        let tmp = TempDir::new().unwrap();
        let resolved = canonicalize(tmp.path()).unwrap();
        let as_str = resolved.to_string_lossy();
        assert!(
            !as_str.starts_with(r"\\?\"),
            "verbatim prefix must be stripped so the path is shell-usable, got: {as_str}"
        );
    }

    #[test]
    fn missing_path_is_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = canonicalize(&tmp.path().join("does-not-exist")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
