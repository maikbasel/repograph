//! Turning a repo's git-tracked files into indexable chunks.
//!
//! Chunking is deliberately language-agnostic: a file is split into bounded
//! windows of lines with a small overlap, each carrying a contextual prefix
//! (`repo › relpath › Lstart-end`) so both the lexical index and the embedding
//! model see where a chunk came from. Tree-sitter symbol-aware chunking is a
//! later change; this keeps v1 shippable across every language.

use std::path::Path;

use git2::{ObjectType, Repository};

/// Maximum file size we index, in bytes.
///
/// Matches codegraph's `maxFileSize` guard — larger files are almost always
/// vendored assets, minified bundles, or generated blobs that pollute
/// retrieval without adding signal.
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// Number of lines per chunk window.
pub const CHUNK_LINES: usize = 40;

/// Lines of overlap between consecutive chunks, so a construct that straddles a
/// window boundary still appears whole in at least one chunk.
pub const CHUNK_OVERLAP: usize = 10;

/// One indexable unit: a window of lines from a single file plus the metadata
/// the store and renderer need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Repo-relative path, forward-slashed.
    pub path: String,
    /// 1-based start line of the window (inclusive).
    pub start_line: u32,
    /// 1-based end line of the window (inclusive).
    pub end_line: u32,
    /// Raw source lines — the snippet shown to the user.
    pub content: String,
    /// Contextual prefix prepended before lexical indexing / embedding.
    pub prefix: String,
}

impl Chunk {
    /// The text fed to the lexical index and the embedding model: the
    /// contextual prefix followed by the raw content.
    #[must_use]
    pub fn index_text(&self) -> String {
        format!("{}\n{}", self.prefix, self.content)
    }
}

/// A git-tracked file resolved to its current working-tree bytes, ready to
/// chunk.
///
/// `content_hash` is the git blob SHA of `text`, used to detect changes for
/// incremental reindexing. `mtime_unix` is the working-tree modification time
/// (unix seconds), aggregated into the per-repo staleness baseline.
#[derive(Debug, Clone)]
pub struct TrackedFile {
    pub path: String,
    pub content_hash: String,
    pub text: String,
    pub mtime_unix: i64,
}

/// Working-tree modification time of `meta` as unix seconds. Falls back to `0`
/// when the platform/filesystem cannot report it — a stable floor that simply
/// makes the file never look "newer than baseline" on its own.
fn mtime_unix(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Enumerate the git-tracked files of `repo_path` eligible for indexing.
///
/// Eligible means tracked (present in the git index), under [`MAX_FILE_BYTES`],
/// and valid UTF-8. Ignored and untracked files are excluded by construction —
/// only index entries are walked. Files staged-deleted (gone from the working
/// tree) are skipped.
///
/// `repo` is the already-opened repository; the caller owns it so HEAD can be
/// inspected separately for the indexed-commit record.
///
/// # Errors
///
/// Returns the underlying [`git2::Error`] when the index cannot be read.
pub fn tracked_files(repo: &Repository, repo_path: &Path) -> Result<Vec<TrackedFile>, git2::Error> {
    let index = repo.index()?;
    let mut out = Vec::new();
    for i in 0..index.len() {
        let Some(entry) = index.get(i) else {
            continue;
        };
        let Ok(rel) = std::str::from_utf8(&entry.path) else {
            continue; // non-UTF-8 path: skip rather than guess an encoding.
        };
        let rel = rel.replace('\\', "/");
        let abs = repo_path.join(&rel);
        let Ok(meta) = std::fs::metadata(&abs) else {
            continue; // staged-deleted or unreadable: nothing to index.
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&abs) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue; // binary / non-UTF-8 content: not searchable text.
        };
        let content_hash = blob_hash(text.as_bytes());
        out.push(TrackedFile {
            path: rel,
            content_hash,
            text,
            mtime_unix: mtime_unix(&meta),
        });
    }
    Ok(out)
}

/// Newest working-tree modification time (unix seconds) across `repo`'s
/// git-tracked files, or `None` when nothing eligible is tracked.
///
/// This is the cheap staleness probe for auto-refresh: it `stat`s each tracked
/// entry but **never reads or hashes** file contents, mirroring
/// [`tracked_files`]'s tracked/size eligibility so it agrees with what would
/// actually be indexed.
///
/// # Errors
///
/// Returns the underlying [`git2::Error`] when the index cannot be read.
pub fn tracked_mtimes(repo: &Repository, repo_path: &Path) -> Result<Option<i64>, git2::Error> {
    let index = repo.index()?;
    let mut newest: Option<i64> = None;
    for i in 0..index.len() {
        let Some(entry) = index.get(i) else {
            continue;
        };
        let Ok(rel) = std::str::from_utf8(&entry.path) else {
            continue;
        };
        let abs = repo_path.join(rel.replace('\\', "/"));
        let Ok(meta) = std::fs::metadata(&abs) else {
            continue; // staged-deleted or unreadable: nothing to index.
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let m = mtime_unix(&meta);
        newest = Some(newest.map_or(m, |n| n.max(m)));
    }
    Ok(newest)
}

/// Git blob SHA of `bytes` — the same identity git uses for file content. Reused
/// as the incremental-reindex change key so no extra hashing dependency is
/// needed. Falls back to a length tag only if libgit2 cannot hash (it does not
/// touch the object database, so this effectively never fails).
fn blob_hash(bytes: &[u8]) -> String {
    git2::Oid::hash_object(ObjectType::Blob, bytes)
        .map_or_else(|_| format!("len:{}", bytes.len()), |oid| oid.to_string())
}

/// Split a file's `text` into overlapping line-window [`Chunk`]s. An empty or
/// whitespace-only file yields no chunks.
#[must_use]
pub fn chunk_file(repo: &str, path: &str, text: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Vec::new();
    }
    let stride = CHUNK_LINES.saturating_sub(CHUNK_OVERLAP).max(1);
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + CHUNK_LINES).min(lines.len());
        let content = lines[start..end].join("\n");
        if !content.trim().is_empty() {
            let start_line = u32::try_from(start + 1).unwrap_or(u32::MAX);
            let end_line = u32::try_from(end).unwrap_or(u32::MAX);
            let prefix = format!("{repo} › {path} › L{start_line}-{end_line}");
            chunks.push(Chunk {
                path: path.to_string(),
                start_line,
                end_line,
                content,
                prefix,
            });
        }
        if end == lines.len() {
            break;
        }
        start += stride;
    }
    chunks
}

#[cfg(test)]
mod tests {
    // Tests build fixtures with literal sizes/line counts; the lossless-cast
    // and format-collect lints are noise here.
    #![allow(
        clippy::unwrap_used,
        clippy::cast_possible_truncation,
        clippy::format_collect
    )]
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn init_repo_with(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("r");
        std::fs::create_dir_all(&dir).unwrap();
        let repo = git2::Repository::init(&dir).unwrap();
        for (rel, body) in files {
            let abs = dir.join(rel);
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&abs, body).unwrap();
        }
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        (tmp, dir)
    }

    #[test]
    fn tracked_files_returns_added_text_files() {
        let (_tmp, dir) = init_repo_with(&[("src/a.rs", "fn a() {}\n"), ("README.md", "# hi\n")]);
        let repo = git2::Repository::open(&dir).unwrap();
        let mut files = tracked_files(&repo, &dir).unwrap();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["README.md", "src/a.rs"]);
        assert!(files.iter().all(|f| !f.content_hash.is_empty()));
    }

    #[test]
    fn tracked_mtimes_reports_newest_and_ignores_untracked() {
        let (_tmp, dir) = init_repo_with(&[("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n")]);
        let repo = git2::Repository::open(&dir).unwrap();
        let baseline = tracked_mtimes(&repo, &dir).unwrap().unwrap();

        // Stamp one tracked file into the future → newest mtime moves up.
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        std::fs::File::options()
            .write(true)
            .open(dir.join("b.rs"))
            .unwrap()
            .set_modified(future)
            .unwrap();
        let bumped = tracked_mtimes(&repo, &dir).unwrap().unwrap();
        assert!(bumped > baseline, "newest tracked mtime tracks the edit");

        // An untracked file, however new, does not affect the result.
        let untracked = dir.join("scratch.tmp");
        std::fs::write(&untracked, "x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&untracked)
            .unwrap()
            .set_modified(future + std::time::Duration::from_secs(120))
            .unwrap();
        assert_eq!(
            tracked_mtimes(&repo, &dir).unwrap().unwrap(),
            bumped,
            "untracked files are excluded from the staleness probe"
        );
    }

    #[test]
    fn tracked_files_excludes_untracked() {
        let (_tmp, dir) = init_repo_with(&[("tracked.rs", "fn t() {}\n")]);
        std::fs::write(dir.join("untracked.rs"), "fn u() {}\n").unwrap();
        let repo = git2::Repository::open(&dir).unwrap();
        let files = tracked_files(&repo, &dir).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["tracked.rs"]);
    }

    #[test]
    fn tracked_files_skips_oversize_and_binary() {
        let big = "x".repeat((MAX_FILE_BYTES + 1) as usize);
        let (_tmp, dir) = init_repo_with(&[
            ("ok.txt", "small\n"),
            ("big.txt", big.as_str()),
            ("bin.dat", "\u{0}"),
        ]);
        // Replace bin.dat content with real non-UTF-8 bytes after add.
        std::fs::write(dir.join("bin.dat"), [0xff, 0xfe, 0x00]).unwrap();
        let repo = git2::Repository::open(&dir).unwrap();
        let files = tracked_files(&repo, &dir).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["ok.txt"], "oversize + binary skipped");
    }

    #[test]
    fn blob_hash_matches_git_blob_identity() {
        // The empty blob SHA is a well-known git constant.
        assert_eq!(blob_hash(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn chunk_file_windows_with_overlap_and_prefix() {
        let body: String = (1..=100).map(|n| format!("line{n}\n")).collect();
        let chunks = chunk_file("repo", "src/big.rs", &body);
        assert!(chunks.len() > 1, "long file splits into multiple chunks");
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, CHUNK_LINES as u32);
        // Second window starts at stride+1 (overlap retained).
        let stride = (CHUNK_LINES - CHUNK_OVERLAP) as u32;
        assert_eq!(chunks[1].start_line, stride + 1);
        assert!(chunks[0].prefix.contains("repo › src/big.rs › L1-"));
        assert!(chunks[0].index_text().starts_with("repo › src/big.rs"));
    }

    #[test]
    fn chunk_file_empty_yields_nothing() {
        assert!(chunk_file("r", "empty.txt", "   \n\n").is_empty());
    }

    #[test]
    fn chunk_file_short_file_is_single_chunk() {
        let chunks = chunk_file("r", "a.rs", "fn a() {}\nfn b() {}\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 2);
    }
}
