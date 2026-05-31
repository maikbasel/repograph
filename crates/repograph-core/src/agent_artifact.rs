//! Per-agent native instruction artifacts.
//!
//! Installs a small file at a well-known path for each selected agent so the
//! agent's runtime picks it up automatically and learns when to invoke
//! `repograph` CLI commands.
//!
//! ## Surface
//!
//! - [`Scope`] — user-scope vs project-scope target root.
//! - [`ArtifactResult`] — per-agent outcome of an install (Written, Unchanged,
//!   Skipped, Failed).
//! - [`BODY`] — the canonical instructional prose, shared across every
//!   per-agent writer so the CLI surface is documented in exactly one place.
//! - [`install_artifacts`] — entry point that iterates a selection and returns
//!   one result per agent in selection order.
//!
//! ## Delimiter contract
//!
//! Each artifact wraps the canonical body in [`DELIMITER_BEGIN`] /
//! [`DELIMITER_END`] HTML comments. This lets a single file mix user-authored
//! content with the repograph-managed block (relevant for `AGENTS.md` /
//! `CONVENTIONS.md`, which users may already maintain). Re-runs only touch
//! the delimited region; everything outside is byte-preserved.
//!
//! ## Force-bypass
//!
//! Passing `force = true` to the install layer skips the delimiter check and
//! writes the file fresh with only the delimited block. Any prior file
//! contents (including user content outside the delimited region) are
//! discarded. This is the escape hatch for re-asserting the canonical body
//! after local edits drift.
//!
//! ## Skipped agents
//!
//! Not every selected agent has a writer. [`AgentId::Copilot`] is deferred in
//! v1 because its instruction format varies across surfaces (repo-level,
//! editor-level, Copilot Workspace) and no single converged path covers them.
//! Selecting Copilot is fine — it just produces a [`ArtifactResult::Skipped`]
//! with no file write.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agents::AgentId;
use crate::error::RepographError;

/// Where on disk an artifact should be installed.
///
/// `User` resolves to a path under the host's home directory; `Project`
/// resolves to a path under the current working directory. Some agents are
/// project-scope only by convention; for them, `User` silently falls through
/// to the project path (see [`scope_is_meaningful`] and the v1 matrix in
/// [`resolve_path`]).
///
/// Named `Scope` and kept at module scope (not re-exported at the crate root)
/// to coexist with the existing `repograph_core::context::Scope`, which is a
/// different concept (context-aggregation scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Install under the host's home directory (`~/`).
    User,
    /// Install under the current working directory (the project root).
    Project,
}

/// Per-agent outcome of an install. The orchestrator returns one of these per
/// input agent in selection order.
///
/// `RepographError` is not `Clone`, so this enum is `Debug`-only on purpose.
#[derive(Debug)]
pub enum ArtifactResult {
    /// File was created or its delimited block was rewritten.
    Written { agent: AgentId, path: PathBuf },
    /// File already exists with a delimited block whose body is byte-identical
    /// to the canonical content; no I/O write occurred.
    Unchanged { agent: AgentId, path: PathBuf },
    /// Agent has no writer (today: only `Copilot`); the install layer skipped
    /// it with no file write attempted.
    Skipped {
        agent: AgentId,
        reason: &'static str,
    },
    /// Per-agent failure (read or write I/O error). Reported on stderr; does
    /// not abort the surrounding run.
    Failed {
        agent: AgentId,
        error: RepographError,
    },
}

impl ArtifactResult {
    /// The agent this result pertains to. Useful in summary logs and tests.
    #[must_use]
    pub const fn agent(&self) -> AgentId {
        match self {
            Self::Written { agent, .. }
            | Self::Unchanged { agent, .. }
            | Self::Skipped { agent, .. }
            | Self::Failed { agent, .. } => *agent,
        }
    }
}

/// Reason strings used in [`ArtifactResult::Skipped`]. Stable: agents may
/// observe them in `repograph doctor` output or log scraping.
pub const REASON_COPILOT_DEFERRED: &str = "no writer in v1";

/// HTML-comment marker opening the repograph-managed region of an artifact.
pub const DELIMITER_BEGIN: &str = "<!-- repograph:begin -->";

/// HTML-comment marker closing the repograph-managed region of an artifact.
pub const DELIMITER_END: &str = "<!-- repograph:end -->";

/// One-line description used in every per-agent frontmatter block (Claude
/// `SKILL.md`, Cursor `.mdc`) and in the `# repograph` heading body for the
/// frontmatter-less writers.
pub const SUMMARY: &str = "Cross-repo context for AI agents";

/// The single canonical instructional body, shared by every per-agent writer.
///
/// Owned by `repograph-core` so the CLI surface is documented in exactly one
/// place. Per-agent writers (see [`render_artifact`]) wrap this string in
/// native-format frontmatter or headers but never edit its content.
///
/// Content stability: this string is byte-stable for a given crate version. A
/// body update bumps the in-file content for users on re-`init`; the spliced
/// install layer rewrites only the delimited region.
pub const BODY: &str = include_str!("agent_artifact_body.md");

/// Convenience accessor for the writer-side summary. Mirrors `SUMMARY`; this
/// exists so writers don't reach into module-level constants directly.
#[must_use]
pub const fn writer_summary() -> &'static str {
    SUMMARY
}

/// Is there an installed-artifact writer for this agent in v1?
///
/// `Copilot` returns `false` because its instruction format varies across
/// surfaces and no single converged path exists today (see module docs).
/// Every other v1 agent returns `true`.
#[must_use]
pub const fn has_artifact_writer(agent: AgentId) -> bool {
    !matches!(agent, AgentId::Copilot)
}

/// Does this agent's artifact occupy the whole file (frontmatter included),
/// with no expectation of pre-existing user content to preserve?
///
/// `true` for `claude-code` (`SKILL.md` is wholly repograph's) and `cursor`
/// (`.cursor/rules/repograph.mdc` is rule-engine-specific). For these agents
/// the install layer writes the full [`render_artifact`] output — including
/// the YAML frontmatter — rather than splicing only the delimited region.
///
/// `false` for `agents-md`, `aider`, and `windsurf`, whose target files may
/// already contain user-authored prose that the install layer must preserve
/// outside the delimited block.
#[must_use]
pub const fn wholly_owned_file(agent: AgentId) -> bool {
    matches!(agent, AgentId::ClaudeCode | AgentId::Cursor)
}

/// Resolve the target install path for `(agent, scope)`.
///
/// Pass `home` and `cwd` explicitly so callers (and tests) control where the
/// roots come from — this module never calls `dirs::home_dir()` or
/// `std::env::current_dir()` itself.
///
/// Agents whose path is project-only by convention (AGENTS.md, CONVENTIONS.md,
/// Cursor `.cursor/rules/*`) ignore `Scope::User` and return the project path.
/// See [`scope_is_meaningful`] for the symmetric predicate the init command
/// uses to decide whether to require a `--scope` flag under `--no-prompt`.
#[must_use]
pub fn resolve_path(agent: AgentId, scope: Scope, home: &Path, cwd: &Path) -> PathBuf {
    match agent {
        AgentId::ClaudeCode => match scope {
            Scope::User => home.join(".claude/skills/repograph/SKILL.md"),
            Scope::Project => cwd.join(".claude/skills/repograph/SKILL.md"),
        },
        AgentId::AgentsMd | AgentId::Aider | AgentId::Cursor => {
            // Project-only agents: scope falls through to project root.
            match agent {
                AgentId::AgentsMd => cwd.join("AGENTS.md"),
                AgentId::Aider => cwd.join("CONVENTIONS.md"),
                AgentId::Cursor => cwd.join(".cursor/rules/repograph.mdc"),
                _ => unreachable!(),
            }
        }
        AgentId::Windsurf => match scope {
            Scope::User => home.join(".codeium/windsurf/memories/repograph.md"),
            Scope::Project => cwd.join(".windsurfrules"),
        },
        AgentId::Copilot => {
            // `has_artifact_writer` returns false; install layer skips before
            // calling `resolve_path`. Returning a path here would mislead.
            unreachable!("resolve_path: copilot has no writer; check has_artifact_writer first")
        }
    }
}

/// Does the choice between `Scope::User` and `Scope::Project` change the
/// resolved path for this agent?
///
/// `false` for project-only agents (their user path equals their project path)
/// and for agents without a writer. The init command uses this to decide
/// whether `--scope` is required under `--no-prompt`.
#[must_use]
pub fn scope_is_meaningful(agent: AgentId) -> bool {
    if !has_artifact_writer(agent) {
        return false;
    }
    // Compare paths using two distinct dummy roots so we detect a real
    // dependency on `scope`. If the resolver returns the same path under both,
    // scope doesn't matter for this agent.
    let home = Path::new("/__home__");
    let cwd = Path::new("/__cwd__");
    resolve_path(agent, Scope::User, home, cwd) != resolve_path(agent, Scope::Project, home, cwd)
}

/// Compose the full file contents for `agent`: per-agent frontmatter (if any)
/// followed by the managed-section delimiters wrapping [`BODY`], plus a
/// trailing newline.
///
/// Centralizes the wrapping logic so every install path produces byte-stable,
/// deterministic output (no timestamps, no host-specific strings).
///
/// # Panics
///
/// Panics with `unreachable!` if called for `AgentId::Copilot`. Callers MUST
/// gate on [`has_artifact_writer`] first; reaching this branch is a logic bug.
#[must_use]
pub fn render_artifact(agent: AgentId) -> String {
    match agent {
        AgentId::ClaudeCode => format!(
            "---\nname: repograph\ndescription: {summary}\n---\n\n\
             {begin}\n{body}\n{end}\n",
            summary = writer_summary(),
            begin = DELIMITER_BEGIN,
            body = BODY,
            end = DELIMITER_END,
        ),
        AgentId::Cursor => format!(
            "---\ndescription: {summary}\nglobs: []\n---\n\n\
             {begin}\n{body}\n{end}\n",
            summary = writer_summary(),
            begin = DELIMITER_BEGIN,
            body = BODY,
            end = DELIMITER_END,
        ),
        AgentId::AgentsMd | AgentId::Aider | AgentId::Windsurf => {
            format!("{DELIMITER_BEGIN}\n# repograph\n\n{BODY}\n{DELIMITER_END}\n")
        }
        AgentId::Copilot => {
            unreachable!("render_artifact: copilot has no writer; check has_artifact_writer first")
        }
    }
}

/// Outcome of [`splice_managed_section`] — describes how the install layer
/// should reconcile the new body against the existing file contents.
#[derive(Debug, PartialEq, Eq)]
pub enum SpliceOutcome {
    /// Existing file contains the delimited block and its inner body matches
    /// the new body byte-for-byte. No write needed.
    Identical,
    /// Existing file contains the delimited block but the inner body differs.
    /// The carried string is the full new file contents — only the delimited
    /// region was rewritten; everything outside is byte-preserved.
    Replaced(String),
    /// Existing file has no delimited block. The carried string is the
    /// existing contents (with a separating newline if non-empty) plus a
    /// freshly-appended delimited block.
    Appended(String),
    /// Existing file does not exist. The carried string is the bare delimited
    /// block.
    FreshWrite(String),
}

/// Pure-string idempotent splice: read the existing file (or `None`), produce
/// the [`SpliceOutcome`] that tells the install layer what to write.
///
/// `new_block_body` is the canonical body that should land *between* the
/// delimiters — typically the full output of [`render_artifact`] minus its
/// frontmatter. For files that always own the whole content (Claude SKILL.md,
/// Cursor .mdc), pass [`render_artifact`] in full; the delimiter pair appears
/// as the entire body and the function still routes correctly.
///
/// I/O-free: testable as a string transformation.
#[must_use]
pub fn splice_managed_section(existing: Option<&str>, new_block_body: &str) -> SpliceOutcome {
    let full_block = format!("{DELIMITER_BEGIN}\n{new_block_body}\n{DELIMITER_END}\n");
    let Some(existing) = existing else {
        return SpliceOutcome::FreshWrite(full_block);
    };

    // Locate the delimiter pair within the existing file.
    if let Some(begin_idx) = existing.find(DELIMITER_BEGIN) {
        // The body starts after the begin-delimiter line.
        let after_begin = begin_idx + DELIMITER_BEGIN.len();
        // Skip a single newline immediately after the begin delimiter, if
        // present, so the captured inner-body string doesn't carry that
        // separator. (We re-add it on emit.)
        let inner_start = if existing[after_begin..].starts_with('\n') {
            after_begin + 1
        } else {
            after_begin
        };
        if let Some(end_rel) = existing[inner_start..].find(DELIMITER_END) {
            // `inner_end` is the index of the first byte of DELIMITER_END.
            let inner_end = inner_start + end_rel;
            // The inner body sits between `inner_start` and `inner_end`.
            // It typically ends with a `\n` we wrote on the last install; we
            // compare the body without that trailing newline so callers don't
            // have to think about it.
            let inner_with_trailing_nl = &existing[inner_start..inner_end];
            let inner = inner_with_trailing_nl
                .strip_suffix('\n')
                .unwrap_or(inner_with_trailing_nl);
            if inner == new_block_body {
                return SpliceOutcome::Identical;
            }
            // Build the replaced output: prefix + DELIMITER_BEGIN + \n + body
            // + \n + DELIMITER_END + suffix (where suffix begins at
            // `inner_end + DELIMITER_END.len()`).
            let suffix_start = inner_end + DELIMITER_END.len();
            let mut out = String::with_capacity(existing.len() + new_block_body.len());
            out.push_str(&existing[..begin_idx]);
            out.push_str(DELIMITER_BEGIN);
            out.push('\n');
            out.push_str(new_block_body);
            out.push('\n');
            out.push_str(DELIMITER_END);
            out.push_str(&existing[suffix_start..]);
            return SpliceOutcome::Replaced(out);
        }
        // Begin without end is malformed; treat as no-block-present and append
        // a fresh block. The user content stays intact.
    }

    // No delimiter pair: append the full block after a separating newline.
    let needs_sep = !existing.is_empty() && !existing.ends_with('\n');
    let mut out = String::with_capacity(existing.len() + full_block.len() + usize::from(needs_sep));
    out.push_str(existing);
    if !existing.is_empty() {
        if needs_sep {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&full_block);
    SpliceOutcome::Appended(out)
}

/// Install a single artifact at `path` for `agent`.
///
/// Reads the existing file (if any), splices the canonical body in via
/// [`splice_managed_section`] (or short-circuits to a fresh write when
/// `force = true`), and writes the result through `fs_err`.
///
/// Returns a typed [`ArtifactResult`]:
///
/// - [`Written`](ArtifactResult::Written) — file created or delimited region
///   updated.
/// - [`Unchanged`](ArtifactResult::Unchanged) — existing file already
///   contained the canonical body byte-for-byte.
/// - [`Failed`](ArtifactResult::Failed) — read or write I/O error. Surrounding
///   orchestration (see [`install_artifacts`]) does not abort on `Failed`.
///
/// Caller MUST gate on [`has_artifact_writer`] first; this function calls
/// [`render_artifact`] which panics for `Copilot`.
#[must_use]
pub fn install_one(agent: AgentId, path: &Path, force: bool) -> ArtifactResult {
    debug_assert!(
        has_artifact_writer(agent),
        "install_one called for an agent without a writer: {agent:?}"
    );

    let full_artifact = render_artifact(agent);

    let existing = if force {
        None
    } else {
        match fs_err::read_to_string(path) {
            Ok(s) => Some(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return ArtifactResult::Failed {
                    agent,
                    error: RepographError::Io(e),
                };
            }
        }
    };

    // Two install models:
    //
    // - Whole-file owners (claude-code SKILL.md, cursor .mdc): repograph owns
    //   the entire file. Write [`render_artifact`] verbatim — including any
    //   YAML frontmatter — and treat byte-identical existing content as
    //   `Unchanged`. The splice contract doesn't apply because there's no
    //   user content to preserve around the delimited region.
    // - Shared-file agents (agents-md, aider, windsurf): the target file may
    //   already contain user-authored prose. Splice the canonical body into
    //   the delimited region and leave everything outside untouched.
    let to_write = if wholly_owned_file(agent) {
        if let Some(ref existing_body) = existing {
            if existing_body == &full_artifact && !force {
                return ArtifactResult::Unchanged {
                    agent,
                    path: path.to_path_buf(),
                };
            }
        }
        full_artifact
    } else {
        let new_block_body = rendered_inner_body(&full_artifact);
        let outcome = splice_managed_section(existing.as_deref(), &new_block_body);
        match outcome {
            SpliceOutcome::Identical if !force => {
                return ArtifactResult::Unchanged {
                    agent,
                    path: path.to_path_buf(),
                };
            }
            SpliceOutcome::Identical => {
                // force=true and content matched: rewrite anyway for the
                // documented `Written` outcome.
                format!("{DELIMITER_BEGIN}\n{new_block_body}\n{DELIMITER_END}\n")
            }
            SpliceOutcome::Replaced(s)
            | SpliceOutcome::Appended(s)
            | SpliceOutcome::FreshWrite(s) => s,
        }
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = fs_err::create_dir_all(parent) {
                return ArtifactResult::Failed {
                    agent,
                    error: RepographError::Io(e),
                };
            }
        }
    }

    match fs_err::write(path, to_write) {
        Ok(()) => ArtifactResult::Written {
            agent,
            path: path.to_path_buf(),
        },
        Err(e) => ArtifactResult::Failed {
            agent,
            error: RepographError::Io(e),
        },
    }
}

/// Extract the inner-body portion of `render_artifact`'s output — what should
/// land between `DELIMITER_BEGIN` and `DELIMITER_END`.
///
/// For agents with frontmatter (Claude SKILL.md, Cursor .mdc) the frontmatter
/// is stripped and the inner delimited body is the rest; for frontmatter-less
/// writers (AGENTS.md, CONVENTIONS.md, .windsurfrules) the inner body is the
/// body between the delimiters in `render_artifact`'s output.
///
/// This indirection exists so the splice contract is uniform: the install
/// layer always treats `new_block_body` as the substring between delimiters,
/// regardless of frontmatter shape.
///
/// Returns the full `rendered` string back as-is if the delimiters can't be
/// located. That can only happen if `render_artifact` is mis-implemented; the
/// install layer would then write a malformed file rather than panic — the
/// next `cargo test` run would surface the regression because every render
/// test asserts the delimiters are present.
fn rendered_inner_body(rendered: &str) -> String {
    let Some(begin_idx) = rendered.find(DELIMITER_BEGIN) else {
        return rendered.to_string();
    };
    let after_begin = begin_idx + DELIMITER_BEGIN.len();
    let inner_start = if rendered[after_begin..].starts_with('\n') {
        after_begin + 1
    } else {
        after_begin
    };
    let Some(end_idx_rel) = rendered[inner_start..].find(DELIMITER_END) else {
        return rendered.to_string();
    };
    let inner = &rendered[inner_start..inner_start + end_idx_rel];
    inner.strip_suffix('\n').unwrap_or(inner).to_string()
}

/// Render frontmatter (if any) and a managed-section block for `agent`, then
/// install it under the resolved `(scope, home, cwd)` path. The result vector
/// has one entry per input agent in selection order.
///
/// Agents without a writer (see [`has_artifact_writer`]) produce
/// [`ArtifactResult::Skipped`] without touching the filesystem. Per-agent
/// errors are captured as [`ArtifactResult::Failed`] and do NOT abort the
/// remaining agents.
///
/// `force = true` overwrites the target file fresh (see module docs).
///
/// This function is log-free by design (`repograph-core` is pure-value domain
/// code per `.claude/rules/logging.md`). The binary-side caller iterates the
/// returned vector and emits one `tracing` line per result on stderr.
#[must_use]
pub fn install_artifacts(
    agents: &[AgentId],
    scope: Scope,
    home: &Path,
    cwd: &Path,
    force: bool,
) -> Vec<ArtifactResult> {
    let mut results = Vec::with_capacity(agents.len());
    for &agent in agents {
        if !has_artifact_writer(agent) {
            results.push(ArtifactResult::Skipped {
                agent,
                reason: REASON_COPILOT_DEFERRED,
            });
            continue;
        }
        let path = resolve_path(agent, scope, home, cwd);
        results.push(install_one(agent, &path, force));
    }
    results
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use tempfile::TempDir;

    // ---- body ----

    mod body {
        use super::*;

        /// Locate the body's "## Commands" section — the table that tells the
        /// agent which commands to invoke. Returns the section text up to the
        /// next `## ` heading or end-of-body. Mutating commands MUST NOT
        /// appear here; negative-guidance prose in the "Things to avoid"
        /// appendix is allowed to name them.
        fn commands_section() -> &'static str {
            let start = BODY
                .find("## Commands")
                .expect("body has a Commands section");
            let after = start + "## Commands".len();
            let end_rel = BODY[after..].find("\n## ").unwrap_or(BODY.len() - after);
            &BODY[start..after + end_rel]
        }

        #[test]
        fn body_does_not_reference_mutating_commands_in_commands_section() {
            let section = commands_section();
            for forbidden in [
                "repograph add",
                "repograph remove",
                "repograph workspace",
                "repograph init",
            ] {
                assert!(
                    !section.contains(forbidden),
                    "Commands section mentions mutating command: {forbidden}\n---\n{section}",
                );
            }
        }

        #[test]
        fn body_mentions_every_required_read_command() {
            for required in [
                "repograph context",
                "repograph list",
                "repograph status",
                "repograph switch",
                "repograph doctor",
            ] {
                assert!(
                    BODY.contains(required),
                    "BODY missing required command reference: {required}",
                );
            }
        }

        #[test]
        fn body_warns_against_running_mutating_commands_automatically() {
            // The "Things to avoid" appendix must remind the agent not to run
            // mutating commands on its own initiative.
            assert!(
                BODY.contains("Do not run mutating commands"),
                "BODY missing the don't-mutate guidance"
            );
        }
    }

    // ---- path matrix ----

    mod path {
        use super::*;

        fn fixed_roots() -> (PathBuf, PathBuf) {
            (PathBuf::from("/home/u"), PathBuf::from("/proj"))
        }

        #[test]
        fn path_matrix_v1() {
            let (home, cwd) = fixed_roots();
            assert_eq!(
                resolve_path(AgentId::ClaudeCode, Scope::User, &home, &cwd),
                PathBuf::from("/home/u/.claude/skills/repograph/SKILL.md"),
            );
            assert_eq!(
                resolve_path(AgentId::ClaudeCode, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.claude/skills/repograph/SKILL.md"),
            );
            assert_eq!(
                resolve_path(AgentId::AgentsMd, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/AGENTS.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Cursor, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.cursor/rules/repograph.mdc"),
            );
            assert_eq!(
                resolve_path(AgentId::Aider, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/CONVENTIONS.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Windsurf, Scope::User, &home, &cwd),
                PathBuf::from("/home/u/.codeium/windsurf/memories/repograph.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Windsurf, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.windsurfrules"),
            );
        }

        #[test]
        fn project_only_agents_fall_through_under_user_scope() {
            let (home, cwd) = fixed_roots();
            for agent in [AgentId::AgentsMd, AgentId::Aider, AgentId::Cursor] {
                assert_eq!(
                    resolve_path(agent, Scope::User, &home, &cwd),
                    resolve_path(agent, Scope::Project, &home, &cwd),
                    "{agent:?} should fall through under Scope::User",
                );
            }
        }

        #[test]
        fn has_artifact_writer_matches_matrix() {
            assert!(!has_artifact_writer(AgentId::Copilot));
            for agent in [
                AgentId::ClaudeCode,
                AgentId::AgentsMd,
                AgentId::Cursor,
                AgentId::Aider,
                AgentId::Windsurf,
            ] {
                assert!(has_artifact_writer(agent), "{agent:?} should have a writer");
            }
        }

        #[test]
        fn scope_is_meaningful_returns_true_only_for_dual_scope_agents() {
            assert!(scope_is_meaningful(AgentId::ClaudeCode));
            assert!(scope_is_meaningful(AgentId::Windsurf));
            assert!(!scope_is_meaningful(AgentId::AgentsMd));
            assert!(!scope_is_meaningful(AgentId::Aider));
            assert!(!scope_is_meaningful(AgentId::Cursor));
            assert!(!scope_is_meaningful(AgentId::Copilot));
        }
    }

    // ---- render ----

    mod render {
        use super::*;

        #[test]
        fn render_artifact_claude_code_has_yaml_frontmatter() {
            let out = render_artifact(AgentId::ClaudeCode);
            assert!(out.starts_with("---\nname: repograph\n"), "got: {out:?}");
            assert!(
                out.contains(&format!("description: {SUMMARY}\n")),
                "summary in frontmatter, got: {out:?}",
            );
            assert!(out.contains(DELIMITER_BEGIN));
            assert!(out.contains(DELIMITER_END));
            assert!(out.contains("repograph context"));
        }

        #[test]
        fn render_artifact_cursor_has_mdc_frontmatter() {
            let out = render_artifact(AgentId::Cursor);
            assert!(out.starts_with("---\ndescription:"), "got: {out:?}");
            assert!(out.contains("globs: []"), "MDC frontmatter, got: {out:?}");
            assert!(out.contains(DELIMITER_BEGIN));
        }

        #[test]
        fn render_artifact_agents_md_has_no_frontmatter() {
            let out = render_artifact(AgentId::AgentsMd);
            let expected_prefix = format!("{DELIMITER_BEGIN}\n# repograph");
            assert!(out.starts_with(&expected_prefix), "got: {out:?}");
            assert!(!out.starts_with("---"), "must not have YAML frontmatter");
        }

        #[test]
        fn render_artifact_aider_and_windsurf_have_no_frontmatter() {
            for agent in [AgentId::Aider, AgentId::Windsurf] {
                let out = render_artifact(agent);
                assert!(
                    out.starts_with(DELIMITER_BEGIN),
                    "{agent:?} should start with the begin-delimiter",
                );
                assert!(!out.starts_with("---"));
            }
        }

        #[test]
        fn render_artifact_is_deterministic() {
            for agent in [
                AgentId::ClaudeCode,
                AgentId::Cursor,
                AgentId::AgentsMd,
                AgentId::Aider,
                AgentId::Windsurf,
            ] {
                let a = render_artifact(agent);
                let b = render_artifact(agent);
                assert_eq!(a, b, "{agent:?} output must be byte-stable across calls");
            }
        }

        #[test]
        #[should_panic(expected = "copilot has no writer")]
        fn render_artifact_copilot_panics() {
            let _ = render_artifact(AgentId::Copilot);
        }
    }

    // ---- splice ----

    mod splice {
        use super::*;

        fn block(inner: &str) -> String {
            format!("{DELIMITER_BEGIN}\n{inner}\n{DELIMITER_END}\n")
        }

        #[test]
        fn fresh_write() {
            let outcome = splice_managed_section(None, "BODY");
            assert_eq!(outcome, SpliceOutcome::FreshWrite(block("BODY")));
        }

        #[test]
        fn identical_returns_identical() {
            let existing = block("BODY");
            let outcome = splice_managed_section(Some(&existing), "BODY");
            assert_eq!(outcome, SpliceOutcome::Identical);
        }

        #[test]
        fn differing_inner_rewrites_block() {
            let existing = block("OLD");
            let outcome = splice_managed_section(Some(&existing), "NEW");
            match outcome {
                SpliceOutcome::Replaced(s) => assert_eq!(s, block("NEW")),
                other => panic!("expected Replaced, got {other:?}"),
            }
        }

        #[test]
        fn no_delimiters_appends() {
            let existing = "# My project\n\nCustom prose.\n";
            let outcome = splice_managed_section(Some(existing), "BODY");
            match outcome {
                SpliceOutcome::Appended(s) => {
                    let expected = format!("{existing}\n{}", block("BODY"));
                    assert_eq!(s, expected);
                }
                other => panic!("expected Appended, got {other:?}"),
            }
        }

        #[test]
        fn user_content_outside_delimiters_preserved() {
            let existing = format!("pre\n{}post\n", block("old"));
            let outcome = splice_managed_section(Some(&existing), "new");
            match outcome {
                SpliceOutcome::Replaced(s) => {
                    assert_eq!(s, format!("pre\n{}post\n", block("new")));
                }
                other => panic!("expected Replaced, got {other:?}"),
            }
        }

        #[test]
        fn empty_existing_file_appends_with_no_leading_newline() {
            let outcome = splice_managed_section(Some(""), "BODY");
            match outcome {
                SpliceOutcome::Appended(s) => assert_eq!(s, block("BODY")),
                other => panic!("expected Appended for empty file, got {other:?}"),
            }
        }

        #[test]
        fn existing_without_trailing_newline_gets_separator() {
            // Existing file: no trailing newline → splice should add one
            // before the block.
            let existing = "no-newline";
            let outcome = splice_managed_section(Some(existing), "BODY");
            match outcome {
                SpliceOutcome::Appended(s) => {
                    assert_eq!(s, format!("no-newline\n\n{}", block("BODY")));
                }
                other => panic!("expected Appended, got {other:?}"),
            }
        }
    }

    // ---- install_one ----

    mod install_one {
        use super::*;

        fn read(path: &Path) -> String {
            fs_err::read_to_string(path).unwrap()
        }

        #[test]
        fn fresh_install_writes_file() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("nested/AGENTS.md");
            let r = install_one(AgentId::AgentsMd, &path, false);
            match r {
                ArtifactResult::Written { path: p, .. } => assert_eq!(p, path),
                other => panic!("expected Written, got {other:?}"),
            }
            assert_eq!(read(&path), render_artifact(AgentId::AgentsMd));
        }

        #[test]
        fn re_run_with_identical_body_returns_unchanged() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("AGENTS.md");
            let _ = install_one(AgentId::AgentsMd, &path, false);
            let first = read(&path);
            let r = install_one(AgentId::AgentsMd, &path, false);
            match r {
                ArtifactResult::Unchanged { .. } => (),
                other => panic!("expected Unchanged on re-run, got {other:?}"),
            }
            assert_eq!(
                read(&path),
                first,
                "file must be byte-stable across re-runs"
            );
        }

        #[test]
        fn force_on_identical_returns_written() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("AGENTS.md");
            let _ = install_one(AgentId::AgentsMd, &path, false);
            let first = read(&path);
            let r = install_one(AgentId::AgentsMd, &path, true);
            match r {
                ArtifactResult::Written { .. } => (),
                other => panic!("expected Written under force, got {other:?}"),
            }
            assert_eq!(
                read(&path),
                first,
                "force on identical content rewrites but byte content is the same"
            );
        }

        #[test]
        fn force_overwrites_user_content() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("AGENTS.md");
            fs_err::write(&path, "# My project\n\nCustom prose.\n").unwrap();
            let r = install_one(AgentId::AgentsMd, &path, true);
            match r {
                ArtifactResult::Written { .. } => (),
                other => panic!("expected Written under force, got {other:?}"),
            }
            let after = read(&path);
            assert!(after.starts_with(DELIMITER_BEGIN), "force replaced content");
            assert!(
                !after.contains("Custom prose."),
                "force dropped user content"
            );
        }

        #[test]
        fn fresh_install_for_whole_file_owner_includes_frontmatter() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("nested/SKILL.md");
            let r = install_one(AgentId::ClaudeCode, &path, false);
            assert!(matches!(r, ArtifactResult::Written { .. }));
            let body = read(&path);
            assert!(
                body.starts_with("---\nname: repograph\n"),
                "claude-code fresh install must include YAML frontmatter, got:\n{body}",
            );
            assert!(body.contains(DELIMITER_BEGIN));
            assert!(body.contains(DELIMITER_END));
        }

        #[test]
        fn re_run_whole_file_owner_is_unchanged() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("SKILL.md");
            let _ = install_one(AgentId::ClaudeCode, &path, false);
            let first = read(&path);
            let r = install_one(AgentId::ClaudeCode, &path, false);
            assert!(matches!(r, ArtifactResult::Unchanged { .. }));
            assert_eq!(read(&path), first);
        }

        #[test]
        fn non_force_preserves_user_content_around_block() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("AGENTS.md");
            fs_err::write(&path, "# My project\n\nCustom prose.\n").unwrap();
            let r = install_one(AgentId::AgentsMd, &path, false);
            assert!(matches!(r, ArtifactResult::Written { .. }));
            let after = read(&path);
            assert!(after.starts_with("# My project\n\nCustom prose.\n"));
            assert!(after.contains(DELIMITER_BEGIN));
            assert!(after.contains(DELIMITER_END));
        }
    }

    // ---- install_artifacts ----

    mod install_artifacts {
        use super::*;

        #[test]
        fn returns_one_result_per_agent_in_order() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            let agents = vec![AgentId::AgentsMd, AgentId::ClaudeCode];
            let results = install_artifacts(&agents, Scope::User, &home, &cwd, false);
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].agent(), AgentId::AgentsMd);
            assert_eq!(results[1].agent(), AgentId::ClaudeCode);
        }

        #[test]
        fn copilot_is_skipped() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            let results = install_artifacts(&[AgentId::Copilot], Scope::User, &home, &cwd, false);
            match &results[0] {
                ArtifactResult::Skipped { agent, reason } => {
                    assert_eq!(*agent, AgentId::Copilot);
                    assert_eq!(*reason, REASON_COPILOT_DEFERRED);
                }
                other => panic!("expected Skipped for Copilot, got {other:?}"),
            }
        }

        #[test]
        fn per_agent_failure_does_not_abort_subsequent_agents() {
            // Strategy: make the AgentsMd target unwritable, then install
            // AgentsMd followed by ClaudeCode. Unix-only (skip on Windows).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let dir = TempDir::new().unwrap();
                let home = dir.path().join("home");
                let cwd = dir.path().join("proj");
                fs_err::create_dir_all(&home).unwrap();
                fs_err::create_dir_all(&cwd).unwrap();
                // Create AGENTS.md as a directory to force the write to fail.
                fs_err::create_dir_all(cwd.join("AGENTS.md")).unwrap();
                let results = install_artifacts(
                    &[AgentId::AgentsMd, AgentId::ClaudeCode],
                    Scope::User,
                    &home,
                    &cwd,
                    false,
                );
                assert_eq!(results.len(), 2);
                assert!(matches!(results[0], ArtifactResult::Failed { .. }));
                assert!(matches!(
                    results[1],
                    ArtifactResult::Written { .. } | ArtifactResult::Unchanged { .. }
                ));
                // Restore mode so TempDir can clean up.
                let mut perms = fs_err::metadata(cwd.join("AGENTS.md"))
                    .unwrap()
                    .permissions();
                perms.set_mode(0o755);
                fs_err::set_permissions(cwd.join("AGENTS.md"), perms).unwrap();
            }
        }

        #[test]
        fn copilot_in_mixed_selection_does_not_block_others() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            let results = install_artifacts(
                &[AgentId::Copilot, AgentId::AgentsMd, AgentId::ClaudeCode],
                Scope::User,
                &home,
                &cwd,
                false,
            );
            assert_eq!(results.len(), 3);
            assert!(matches!(results[0], ArtifactResult::Skipped { .. }));
            assert!(matches!(results[1], ArtifactResult::Written { .. }));
            assert!(matches!(results[2], ArtifactResult::Written { .. }));
        }
    }
}
