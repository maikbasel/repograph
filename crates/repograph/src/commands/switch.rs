//! `repograph switch <name>` — emit `cd <path>` for shell `eval`.
//!
//! Stdout is exactly the bytes `cd <quoted-or-bare-path>\n`. No JSON mode, no
//! TTY/non-TTY branching: the line is shell-eval-safe in every context. Pair
//! with the `rg-cd` shell function in the README to teleport.

use std::io::{self, Write};
use std::path::Path;

use clap::Parser;
use repograph_core::{Config, RepographError};

#[derive(Debug, Parser)]
pub struct Args {
    /// Name of a registered repository to switch to.
    #[arg(value_name = "NAME")]
    pub name: String,
}

/// Resolve `<name>` against the registry and write `cd <path>\n` to stdout.
///
/// Unknown names exit `3`; on a near-miss the stderr message includes a
/// `did you mean: …` hint. Path validity is NOT checked — that's `doctor`'s
/// job, and the user's shell surfaces a missing-dir from `cd` directly.
///
/// # Errors
///
/// Returns [`RepographError::NotFound`] with `kind = "repo"` (exit `3`) when
/// `<name>` is not registered; [`RepographError::Io`] (exit `1`/`4`) when
/// stdout write fails or config read fails.
#[tracing::instrument(skip(args, config_dir), fields(name = %args.name))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!(command = "switch", name = %args.name, "start");

    let config = Config::load(config_dir)?;
    let Some(repo) = config.repos().get(&args.name) else {
        let names: Vec<&str> = config.repos().keys().map(String::as_str).collect();
        let suggestions = suggest(&args.name, &names);
        if !suggestions.is_empty() {
            tracing::error!(suggestions = %suggestions.join(", "), "did you mean: {}?", suggestions.join(", "));
        }
        return Err(RepographError::NotFound {
            kind: "repo",
            name: args.name.clone(),
        });
    };

    let line = format!("cd {}\n", shell_quote(&repo.path));
    let mut stdout = io::stdout().lock();
    stdout.write_all(line.as_bytes())?;
    stdout.flush()?;

    tracing::info!(repo = %args.name, path = %repo.path.display(), "resolved");
    Ok(())
}

/// Set of shell metacharacters that force single-quoting. Whitespace + the
/// POSIX shell special set; conservative on purpose.
const NEEDS_QUOTING: &[char] = &[
    ' ', '\t', '\n', '\'', '"', '$', '\\', '`', '*', '?', '[', ']', '{', '}', '(', ')', ';', '&',
    '|', '<', '>', '!', '#', '~',
];

/// Format `path` as a POSIX shell argument. When the path contains any
/// character in [`NEEDS_QUOTING`], wrap in single quotes and escape embedded
/// `'` as `'\''` (POSIX single-quoted string escape). Otherwise emit unquoted
/// for readability.
fn shell_quote(path: &Path) -> String {
    let s = path.display().to_string();
    if !s.chars().any(|c| NEEDS_QUOTING.contains(&c)) {
        return s;
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Top three near-miss suggestions for `target` against `candidates`. Returns
/// names with `distance <= 2` AND `distance <= len/2` (Cargo / git heuristic
/// — suppresses noise on very short typos). Sorted by `(distance, name)`.
fn suggest(target: &str, candidates: &[&str]) -> Vec<String> {
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .filter_map(|c| {
            let d = levenshtein(target, c);
            let max_dist = (target.len() / 2).clamp(1, 2);
            if d <= 2 && d <= max_dist {
                Some((d, *c))
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(3)
        .map(|(_, name)| name.to_string())
        .collect()
}

/// Iterative two-row Levenshtein distance — small enough to inline instead of
/// pulling in `strsim` for a one-shot use site.
fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    if a_chars.is_empty() {
        return b_chars.len();
    }
    if b_chars.is_empty() {
        return a_chars.len();
    }
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr: Vec<usize> = vec![0; b_chars.len() + 1];
    for (i, ac) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, bc) in b_chars.iter().enumerate() {
            let cost = usize::from(ac != bc);
            curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn shell_quote_unquoted_for_plain_ascii() {
        assert_eq!(
            shell_quote(&PathBuf::from("/home/user/code/api")),
            "/home/user/code/api"
        );
        assert_eq!(shell_quote(&PathBuf::from("/tmp/x")), "/tmp/x");
    }

    #[test]
    fn shell_quote_single_quotes_when_space_present() {
        assert_eq!(
            shell_quote(&PathBuf::from("/tmp/has space/repo")),
            "'/tmp/has space/repo'"
        );
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quote() {
        assert_eq!(
            shell_quote(&PathBuf::from("/tmp/mike's repo")),
            "'/tmp/mike'\\''s repo'"
        );
    }

    #[test]
    fn shell_quote_quotes_dollar_sign() {
        assert_eq!(shell_quote(&PathBuf::from("/tmp/$work")), "'/tmp/$work'");
    }

    #[test]
    fn shell_quote_quotes_tilde() {
        assert_eq!(shell_quote(&PathBuf::from("/tmp/~user")), "'/tmp/~user'");
    }

    #[test]
    fn shell_quote_quotes_backtick() {
        assert_eq!(shell_quote(&PathBuf::from("/tmp/`x")), "'/tmp/`x'");
    }

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("foo", "foo"), 0);
    }

    #[test]
    fn levenshtein_one_edit() {
        assert_eq!(levenshtein("api", "app"), 1);
        assert_eq!(levenshtein("api", "apis"), 1);
    }

    #[test]
    fn levenshtein_two_edits() {
        // delete `a`, substitute `p`→`a`, substitute `i`→`b` → 3 edits; but
        // simpler: substitute `f`→`a`, substitute `o`→`b`, substitute `o`→`c`
        assert_eq!(levenshtein("foo", "abc"), 3);
        // two substitutions
        assert_eq!(levenshtein("api", "abe"), 2);
    }

    #[test]
    fn levenshtein_empty_string() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn suggest_returns_near_miss_within_threshold() {
        let candidates = ["api", "ui", "lib"];
        let s = suggest("app", &candidates);
        assert_eq!(s, vec!["api".to_string()]);
    }

    #[test]
    fn suggest_returns_empty_for_no_near_miss() {
        let candidates = ["api"];
        let s = suggest("zzzz", &candidates);
        assert!(s.is_empty(), "no suggestion when distance > threshold");
    }

    #[test]
    fn suggest_ties_break_by_name_ascending() {
        let candidates = ["api", "app", "aps"];
        let s = suggest("apt", &candidates);
        assert_eq!(
            s,
            vec!["api".to_string(), "app".to_string(), "aps".to_string()]
        );
    }

    #[test]
    fn suggest_truncates_to_three() {
        let candidates = ["api", "apx", "apy", "apz"];
        let s = suggest("ap", &candidates);
        assert_eq!(s.len().min(3), s.len(), "max three suggestions");
        assert!(s.len() <= 3);
    }

    #[test]
    fn suggest_short_typo_with_only_long_candidates_returns_empty() {
        // target length 1 → max_dist = 1; "api" is distance 2 away → filtered
        let candidates = ["api"];
        let s = suggest("z", &candidates);
        assert!(
            s.is_empty(),
            "short typo gates noisy suggestions against long names"
        );
    }
}
