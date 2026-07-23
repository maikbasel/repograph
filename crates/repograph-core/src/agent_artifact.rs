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

/// Which generated skill a given artifact carries.
///
/// `Consumer` is the read-only surface (`list`/`status`/`context`/`switch`);
/// `Setup` is the mutating surface (`add`/`remove`/`edit`/`workspace …`).
/// Wholly-owned-file agents (Claude, Cursor) receive one artifact per
/// capability; flat-file agents inline both capabilities into a single block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// The read-only consumer skill (`repograph`).
    Consumer,
    /// The mutating registry-management skill (`repograph-setup`).
    Setup,
}

impl Capability {
    /// The skill name (and frontmatter `name:`) for this capability.
    #[must_use]
    pub const fn skill_name(self) -> &'static str {
        match self {
            Self::Consumer => "repograph",
            Self::Setup => "repograph-setup",
        }
    }
}

/// Per-artifact outcome of an install. The orchestrator returns one of these
/// per (agent, capability) artifact actually targeted, in selection order.
///
/// `RepographError` is not `Clone`, so this enum is `Debug`-only on purpose.
#[derive(Debug)]
pub enum ArtifactResult {
    /// File was created or its delimited block was rewritten.
    Written {
        agent: AgentId,
        capability: Capability,
        path: PathBuf,
    },
    /// File already exists with a delimited block whose body is byte-identical
    /// to the canonical content; no I/O write occurred.
    Unchanged {
        agent: AgentId,
        capability: Capability,
        path: PathBuf,
    },
    /// Agent has no writer (today: only `Copilot`); the install layer skipped
    /// it with no file write attempted.
    Skipped {
        agent: AgentId,
        reason: &'static str,
    },
    /// Per-artifact failure (read or write I/O error). Reported on stderr; does
    /// not abort the surrounding run.
    Failed {
        agent: AgentId,
        capability: Capability,
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

    /// The capability this result pertains to, or `None` for a `Skipped` agent
    /// (which has no per-capability artifact).
    #[must_use]
    pub const fn capability(&self) -> Option<Capability> {
        match self {
            Self::Written { capability, .. }
            | Self::Unchanged { capability, .. }
            | Self::Failed { capability, .. } => Some(*capability),
            Self::Skipped { .. } => None,
        }
    }
}

/// Reason strings used in [`ArtifactResult::Skipped`]. Stable: agents may
/// observe them in `repograph doctor` output or log scraping.
pub const REASON_COPILOT_DEFERRED: &str = "no writer in v1";

/// Monotonic version of the managed artifact body.
///
/// Bump this whenever the rendered body content changes so installed artifacts
/// can be detected as stale (see [`installed_version`] and the `doctor`
/// freshness check). Kept in sync with the literal in [`DELIMITER_BEGIN`] by a
/// unit test.
pub const ARTIFACT_BODY_VERSION: u32 = 1;

/// Version-agnostic prefix of the begin marker. Splice detection matches on
/// this so an older-version block is recognized and rewritten in place rather
/// than appended as a duplicate.
pub const DELIMITER_BEGIN_PREFIX: &str = "<!-- repograph:begin";

/// HTML-comment marker opening the repograph-managed region of an artifact,
/// carrying the current [`ARTIFACT_BODY_VERSION`] stamp.
pub const DELIMITER_BEGIN: &str = "<!-- repograph:begin v1 -->";

/// HTML-comment marker closing the repograph-managed region of an artifact.
pub const DELIMITER_END: &str = "<!-- repograph:end -->";

/// Parse the body-version stamp from an installed file's managed block.
///
/// Returns `None` when the file has no recognizable begin marker. Used by
/// `doctor` to compare an installed artifact against the running binary's
/// [`ARTIFACT_BODY_VERSION`] without rewriting anything.
#[must_use]
pub fn installed_version(existing: &str) -> Option<u32> {
    let begin = existing.find(DELIMITER_BEGIN_PREFIX)?;
    let after_prefix = &existing[begin + DELIMITER_BEGIN_PREFIX.len()..];
    // Marker shape: ` v<N> -->`. Take up to the closing `-->`, find the `v<N>`.
    let line_end = after_prefix.find("-->")?;
    let marker_tail = &after_prefix[..line_end];
    let token = marker_tail
        .split_whitespace()
        .find(|t| t.starts_with('v'))?;
    token[1..].parse().ok()
}

/// Skill `description` rendered into the YAML frontmatter of the agents that
/// have it (Claude `SKILL.md`, Cursor `.mdc`).
///
/// This string is the *only* signal the host sees when deciding whether to
/// invoke the skill — the body (`BODY`) is loaded only *after* invocation, so
/// trigger phrasing must live here. It therefore leads with concrete user
/// phrasings ("switch to", "what's dirty", …) and an explicit prefer-over-
/// `find`/`git` instruction rather than an abstract one-liner.
///
/// Rendered as a folded block scalar (`>-`, see [`render_artifact`]) so it can
/// safely contain colons, quotes, and slashes without YAML-escaping. Keep it
/// under the 1024-char skill-spec limit. Frontmatter-less writers (AGENTS.md,
/// CONVENTIONS.md, .windsurfrules) use a `# repograph` heading instead and do
/// not embed this string.
pub const SUMMARY: &str = "Use when the user refers to one of their own git projects/repos by name and wants to act on it: switch / open / \"cd into\" a repo (\"switch to taverne\", \"open the api repo\", \"cd into <name>\"), list or compare their registered repos, check cross-repo git status (\"what's dirty\", \"what's in flight across my projects\", \"which repos have uncommitted changes\"), or pull a repo's CLAUDE.md / AGENTS.md content into the conversation. Maintains a local registry of git repositories and exposes their paths, branches, status, and agent docs as structured JSON. ALWAYS prefer this over manual `find` / `git` to resolve a named project to a filesystem path. Use it for which-repo / across-repos questions, not for the current directory's own `git status` (use plain `git` for that).";

/// Skill `description` for the `repograph-setup` capability — the mutating
/// surface.
///
/// Distinct from [`SUMMARY`]: its triggers name registering, grouping, and
/// updating registry entries so the host invokes it (not the read-only consumer
/// skill) when the user wants to change their registry. Rendered into the setup
/// artifact's frontmatter for wholly-owned-file agents.
pub const SETUP_SUMMARY: &str = "Use when the user wants to set up or change their repograph registry: register a local git repo (\"add this repo\", \"track /path/to/project\"), group repos into a workspace (\"create a workspace for acme\", \"put api and web together\"), update an existing entry (\"rename that repo\", \"change its description\", \"retag it\", \"point it at the new path\"), or deregister a repo or workspace (\"remove that repo\", \"delete the acme workspace\"). Drives the mutating commands `add`, `edit`, `remove`, and `workspace …` behind a plan→confirm→execute→verify workflow. Use this for changing the registry; use the read-only `repograph` skill for resolving, listing, or reading it.";

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

/// The canonical instructional body for the `repograph-setup` capability — the
/// mutating surface.
///
/// Owned by `repograph-core` so the CLI mutation surface is documented in
/// exactly one place, mirroring [`BODY`] for the consumer skill.
pub const SETUP_BODY: &str = include_str!("agent_artifact_setup_body.md");

/// Short, **always-loaded** discovery nudge spliced into Claude Code's
/// `CLAUDE.md`.
///
/// The generated `SKILL.md` files are *lazy-loaded*: Claude Code only sees a
/// skill's `description` up front and loads [`BODY`] after deciding to invoke
/// it. That decision is exactly what fails to happen unless the user asks by
/// name. `CLAUDE.md`, by contrast, is read into context every turn — so this
/// terse pointer (trigger phrasings + skill names + prefer-over-`find`/`git`)
/// is what actually makes the host reach for the skill reflexively. Kept short
/// on purpose: it is a signpost to the skills, not a second copy of the body.
pub const POINTER: &str = "## repograph\n\nThis project is registered with **repograph** — a local registry of the user's own git repositories, exposed to agents as structured JSON.\n\nWhen the user refers to one of their registered projects by name, prefer resolving it through repograph over manual `find` / `git`:\n\n- Read-only questions — \"switch to <name>\", \"open the api repo\", \"cd into <name>\", \"what's dirty across my projects\", \"which repos have uncommitted changes\", \"pull in <repo>'s CLAUDE.md\", or searching code across repos — use the **repograph** skill (or run `repograph list` / `status` / `context` / `switch` / `find` directly).\n- Changing the registry — register, group into a workspace, rename, retag, or remove — use the **repograph-setup** skill.\n\nThis is for which-repo / across-repos questions. For the current directory's own state, use plain `git`.";

/// The instructional body for `capability`.
#[must_use]
pub const fn body_for(capability: Capability) -> &'static str {
    match capability {
        Capability::Consumer => BODY,
        Capability::Setup => SETUP_BODY,
    }
}

/// The frontmatter `description:` summary for `capability`.
#[must_use]
pub const fn summary_for(capability: Capability) -> &'static str {
    match capability {
        Capability::Consumer => SUMMARY,
        Capability::Setup => SETUP_SUMMARY,
    }
}

/// Convenience accessor for the consumer writer-side summary. Mirrors `SUMMARY`;
/// this exists so writers don't reach into module-level constants directly.
#[must_use]
pub const fn writer_summary() -> &'static str {
    SUMMARY
}

/// The capabilities that should be emitted for `agent`, in install order.
///
/// Wholly-owned-file agents (Claude, Cursor) emit a discrete artifact per
/// capability. Flat-file agents (AGENTS.md, Aider, Windsurf) inline both bodies
/// into a single block, so they emit one combined artifact tagged `Consumer`.
#[must_use]
pub const fn capabilities_for(agent: AgentId) -> &'static [Capability] {
    if wholly_owned_file(agent) {
        &[Capability::Consumer, Capability::Setup]
    } else {
        &[Capability::Consumer]
    }
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
pub fn resolve_path(
    agent: AgentId,
    capability: Capability,
    scope: Scope,
    home: &Path,
    cwd: &Path,
) -> PathBuf {
    // Flat-file agents (AGENTS.md, Aider, Windsurf) inline both capabilities
    // into one file, so their path is capability-independent. Wholly-owned-file
    // agents get a discrete path per capability, keyed by the skill name.
    let skill = capability.skill_name();
    match agent {
        AgentId::ClaudeCode => {
            let rel = format!(".claude/skills/{skill}/SKILL.md");
            match scope {
                Scope::User => home.join(rel),
                Scope::Project => cwd.join(rel),
            }
        }
        AgentId::Cursor => cwd.join(format!(".cursor/rules/{skill}.mdc")),
        AgentId::AgentsMd => cwd.join("AGENTS.md"),
        AgentId::Aider => cwd.join("CONVENTIONS.md"),
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

/// Resolve the target `CLAUDE.md` for the always-loaded [`POINTER`].
///
/// Project scope targets the repo-root `CLAUDE.md`; user scope targets the
/// global `~/.claude/CLAUDE.md`. Both are files Claude Code loads into context
/// every turn (unlike the lazy-loaded `SKILL.md`), which is the whole point.
#[must_use]
pub fn resolve_pointer_path(scope: Scope, home: &Path, cwd: &Path) -> PathBuf {
    match scope {
        Scope::User => home.join(".claude/CLAUDE.md"),
        Scope::Project => cwd.join("CLAUDE.md"),
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
    // Scope-dependence is identical across capabilities; Consumer is representative.
    resolve_path(agent, Capability::Consumer, Scope::User, home, cwd)
        != resolve_path(agent, Capability::Consumer, Scope::Project, home, cwd)
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
pub fn render_artifact(agent: AgentId, capability: Capability) -> String {
    match agent {
        AgentId::ClaudeCode => format!(
            "---\nname: {name}\ndescription: >-\n  {summary}\n---\n\n\
             {begin}\n{body}\n{end}\n",
            name = capability.skill_name(),
            summary = summary_for(capability),
            begin = DELIMITER_BEGIN,
            body = body_for(capability),
            end = DELIMITER_END,
        ),
        AgentId::Cursor => format!(
            "---\ndescription: >-\n  {summary}\nglobs: []\n---\n\n\
             {begin}\n{body}\n{end}\n",
            summary = summary_for(capability),
            begin = DELIMITER_BEGIN,
            body = body_for(capability),
            end = DELIMITER_END,
        ),
        AgentId::AgentsMd | AgentId::Aider | AgentId::Windsurf => {
            // Flat-file agents inline BOTH capabilities into one managed block:
            // the consumer body followed by the setup body. `capability` is
            // ignored — these agents only ever request the single combined file.
            format!(
                "{DELIMITER_BEGIN}\n# repograph\n\n{BODY}\n\n# repograph-setup\n\n{SETUP_BODY}\n{DELIMITER_END}\n"
            )
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

    // Locate the begin marker by its version-agnostic prefix, so an
    // older-version block (e.g. `… begin v1 …` when the current is `v2`) is
    // still recognized and rewritten in place rather than duplicated.
    if let Some(begin_idx) = existing.find(DELIMITER_BEGIN_PREFIX) {
        // The begin marker spans from `begin_idx` to the end of its `-->`.
        let rest = &existing[begin_idx..];
        if let Some(marker_rel_end) = rest.find("-->") {
            let begin_marker_end = begin_idx + marker_rel_end + "-->".len();
            let matched_begin = &existing[begin_idx..begin_marker_end];
            // The body starts after the begin-marker line.
            // Skip a single newline immediately after the marker, if present.
            let inner_start = if existing[begin_marker_end..].starts_with('\n') {
                begin_marker_end + 1
            } else {
                begin_marker_end
            };
            if let Some(end_rel) = existing[inner_start..].find(DELIMITER_END) {
                // `inner_end` is the index of the first byte of DELIMITER_END.
                let inner_end = inner_start + end_rel;
                // The inner body sits between `inner_start` and `inner_end`.
                // It typically ends with a `\n` we wrote on the last install; we
                // compare the body without that trailing newline so callers
                // don't have to think about it.
                let inner_with_trailing_nl = &existing[inner_start..inner_end];
                let inner = inner_with_trailing_nl
                    .strip_suffix('\n')
                    .unwrap_or(inner_with_trailing_nl);
                // Identical only when both the body AND the marker version match
                // the current ones — a version bump alone forces a rewrite.
                if inner == new_block_body && matched_begin == DELIMITER_BEGIN {
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
        }
        // Begin without end (or without a closing `-->`) is malformed; treat as
        // no-block-present and append a fresh block. User content stays intact.
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
pub fn install_one(
    agent: AgentId,
    capability: Capability,
    path: &Path,
    force: bool,
) -> ArtifactResult {
    debug_assert!(
        has_artifact_writer(agent),
        "install_one called for an agent without a writer: {agent:?}"
    );

    let full_artifact = render_artifact(agent, capability);

    let existing = if force {
        None
    } else {
        match fs_err::read_to_string(path) {
            Ok(s) => Some(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return ArtifactResult::Failed {
                    agent,
                    capability,
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
                    capability,
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
                    capability,
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
                    capability,
                    error: RepographError::Io(e),
                };
            }
        }
    }

    match fs_err::write(path, to_write) {
        Ok(()) => ArtifactResult::Written {
            agent,
            capability,
            path: path.to_path_buf(),
        },
        Err(e) => ArtifactResult::Failed {
            agent,
            capability,
            error: RepographError::Io(e),
        },
    }
}

/// Splice the always-loaded [`POINTER`] into the resolved `CLAUDE.md`.
///
/// Unlike [`install_one`], this **always** splices (reads existing content and
/// rewrites only the delimited region) and never honors a `force` fresh-write —
/// `CLAUDE.md` is wholly user-owned prose that repograph only ever augments,
/// never replaces. Idempotent: a matching block reports `Unchanged`.
///
/// Result is tagged `Capability::Consumer` for logging; the `CLAUDE.md` path in
/// the result distinguishes it from the `SKILL.md` consumer artifact.
#[must_use]
pub fn install_pointer(scope: Scope, home: &Path, cwd: &Path) -> ArtifactResult {
    let path = resolve_pointer_path(scope, home, cwd);
    let existing = match fs_err::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return ArtifactResult::Failed {
                agent: AgentId::ClaudeCode,
                capability: Capability::Consumer,
                error: RepographError::Io(e),
            };
        }
    };

    let to_write = match splice_managed_section(existing.as_deref(), POINTER) {
        SpliceOutcome::Identical => {
            return ArtifactResult::Unchanged {
                agent: AgentId::ClaudeCode,
                capability: Capability::Consumer,
                path,
            };
        }
        SpliceOutcome::Replaced(s) | SpliceOutcome::Appended(s) | SpliceOutcome::FreshWrite(s) => s,
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = fs_err::create_dir_all(parent) {
                return ArtifactResult::Failed {
                    agent: AgentId::ClaudeCode,
                    capability: Capability::Consumer,
                    error: RepographError::Io(e),
                };
            }
        }
    }

    match fs_err::write(&path, to_write) {
        Ok(()) => ArtifactResult::Written {
            agent: AgentId::ClaudeCode,
            capability: Capability::Consumer,
            path,
        },
        Err(e) => ArtifactResult::Failed {
            agent: AgentId::ClaudeCode,
            capability: Capability::Consumer,
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
        // Wholly-owned-file agents emit one artifact per capability (Consumer
        // then Setup); flat-file agents emit a single combined artifact.
        for &capability in capabilities_for(agent) {
            let path = resolve_path(agent, capability, scope, home, cwd);
            results.push(install_one(agent, capability, &path, force));
        }

        // Claude Code loads SKILL.md lazily (description-only up front), so it
        // rarely reaches for the skill unasked. Splice an always-loaded pointer
        // into CLAUDE.md so discovery is reflexive. Claude-only by convention.
        if agent == AgentId::ClaudeCode {
            results.push(install_pointer(scope, home, cwd));
        }
    }
    results
}

/// Refresh every already-installed managed artifact for `selected`, in place.
///
/// This is the engine behind `repograph doctor --fix`: for each selected
/// agent it re-runs the installer against every candidate artifact **whose
/// target file already exists** — at either scope — rewriting only the managed
/// region so a stale block is brought to the current version and a shared file
/// missing the block gets it spliced in. Content outside the managed region is
/// byte-preserved (the installer never uses `force` here).
///
/// Crucially it **never creates a missing artifact**: a target file that does
/// not exist at any scope is skipped, because creating it means choosing a
/// scope — a decision only `repograph init` makes. So `--fix` is a safe,
/// scope-free "update what's installed" that leaves fresh installs to `init`.
///
/// Returns one [`ArtifactResult`] per file actually touched (a `Current` file
/// yields `Unchanged`), in selection order. Log-free, per the core boundary.
#[must_use]
pub fn refresh_installed_artifacts(
    selected: &[AgentId],
    home: &Path,
    cwd: &Path,
) -> Vec<ArtifactResult> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &agent in selected {
        if !has_artifact_writer(agent) {
            continue;
        }
        for &capability in capabilities_for(agent) {
            for scope in [Scope::User, Scope::Project] {
                let path = resolve_path(agent, capability, scope, home, cwd);
                // Scope-insensitive agents resolve both scopes to one path;
                // dedupe so we install it once.
                if !seen.insert(path.clone()) {
                    continue;
                }
                if path.exists() {
                    results.push(install_one(agent, capability, &path, false));
                }
            }
        }

        if agent == AgentId::ClaudeCode {
            for scope in [Scope::User, Scope::Project] {
                let path = resolve_pointer_path(scope, home, cwd);
                if seen.insert(path.clone()) && path.exists() {
                    results.push(install_pointer(scope, home, cwd));
                }
            }
        }
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

        #[test]
        fn consumer_body_delegates_mutation_to_setup_skill() {
            // The don't-mutate guidance must hand off to the setup skill by
            // name, not dead-end at "ask the user".
            assert!(
                BODY.contains("repograph-setup"),
                "consumer BODY must name the repograph-setup skill for mutation"
            );
        }

        #[test]
        fn setup_body_covers_the_mutating_surface() {
            for required in [
                "repograph add",
                "repograph edit",
                "repograph remove",
                "repograph workspace",
            ] {
                assert!(
                    SETUP_BODY.contains(required),
                    "SETUP_BODY missing mutating command reference: {required}",
                );
            }
        }

        #[test]
        fn setup_body_instructs_a_confirm_before_write_workflow() {
            // The plan → confirm → execute → verify discipline must be present.
            for required in ["Plan", "Confirm", "Execute", "Verify"] {
                assert!(
                    SETUP_BODY.contains(required),
                    "SETUP_BODY missing workflow step: {required}",
                );
            }
        }

        #[test]
        fn setup_summary_is_distinct_and_names_mutation_triggers() {
            assert_ne!(SETUP_SUMMARY, SUMMARY, "summaries must differ");
            for trigger in ["register", "workspace", "update"] {
                assert!(
                    SETUP_SUMMARY.contains(trigger),
                    "SETUP_SUMMARY missing trigger phrasing: {trigger}",
                );
            }
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
            let cap = Capability::Consumer;
            assert_eq!(
                resolve_path(AgentId::ClaudeCode, cap, Scope::User, &home, &cwd),
                PathBuf::from("/home/u/.claude/skills/repograph/SKILL.md"),
            );
            assert_eq!(
                resolve_path(AgentId::ClaudeCode, cap, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.claude/skills/repograph/SKILL.md"),
            );
            assert_eq!(
                resolve_path(AgentId::AgentsMd, cap, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/AGENTS.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Cursor, cap, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.cursor/rules/repograph.mdc"),
            );
            assert_eq!(
                resolve_path(AgentId::Aider, cap, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/CONVENTIONS.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Windsurf, cap, Scope::User, &home, &cwd),
                PathBuf::from("/home/u/.codeium/windsurf/memories/repograph.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Windsurf, cap, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.windsurfrules"),
            );
        }

        #[test]
        fn setup_capability_resolves_to_discrete_paths() {
            let (home, cwd) = fixed_roots();
            let cap = Capability::Setup;
            assert_eq!(
                resolve_path(AgentId::ClaudeCode, cap, Scope::User, &home, &cwd),
                PathBuf::from("/home/u/.claude/skills/repograph-setup/SKILL.md"),
            );
            assert_eq!(
                resolve_path(AgentId::Cursor, cap, Scope::Project, &home, &cwd),
                PathBuf::from("/proj/.cursor/rules/repograph-setup.mdc"),
            );
            // Flat-file agents are capability-independent: one shared path.
            assert_eq!(
                resolve_path(AgentId::AgentsMd, cap, Scope::Project, &home, &cwd),
                resolve_path(
                    AgentId::AgentsMd,
                    Capability::Consumer,
                    Scope::Project,
                    &home,
                    &cwd
                ),
            );
        }

        #[test]
        fn project_only_agents_fall_through_under_user_scope() {
            let (home, cwd) = fixed_roots();
            let cap = Capability::Consumer;
            for agent in [AgentId::AgentsMd, AgentId::Aider, AgentId::Cursor] {
                assert_eq!(
                    resolve_path(agent, cap, Scope::User, &home, &cwd),
                    resolve_path(agent, cap, Scope::Project, &home, &cwd),
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
            let out = render_artifact(AgentId::ClaudeCode, Capability::Consumer);
            assert!(out.starts_with("---\nname: repograph\n"), "got: {out:?}");
            assert!(
                out.contains(&format!("description: >-\n  {SUMMARY}\n")),
                "summary rendered as a folded block scalar in frontmatter, got: {out:?}",
            );
            assert!(out.contains(DELIMITER_BEGIN));
            assert!(out.contains(DELIMITER_END));
            assert!(out.contains("repograph context"));
        }

        #[test]
        fn render_artifact_cursor_has_mdc_frontmatter() {
            let out = render_artifact(AgentId::Cursor, Capability::Consumer);
            assert!(out.starts_with("---\ndescription:"), "got: {out:?}");
            assert!(out.contains("globs: []"), "MDC frontmatter, got: {out:?}");
            assert!(out.contains(DELIMITER_BEGIN));
        }

        #[test]
        fn render_artifact_agents_md_has_no_frontmatter() {
            let out = render_artifact(AgentId::AgentsMd, Capability::Consumer);
            let expected_prefix = format!("{DELIMITER_BEGIN}\n# repograph");
            assert!(out.starts_with(&expected_prefix), "got: {out:?}");
            assert!(!out.starts_with("---"), "must not have YAML frontmatter");
            // Flat-file agents inline both capabilities into one block.
            assert!(
                out.contains("# repograph-setup"),
                "AGENTS.md must inline the setup body, got: {out:?}"
            );
        }

        #[test]
        fn render_artifact_aider_and_windsurf_have_no_frontmatter() {
            for agent in [AgentId::Aider, AgentId::Windsurf] {
                let out = render_artifact(agent, Capability::Consumer);
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
                let a = render_artifact(agent, Capability::Consumer);
                let b = render_artifact(agent, Capability::Consumer);
                assert_eq!(a, b, "{agent:?} output must be byte-stable across calls");
            }
        }

        #[test]
        #[should_panic(expected = "copilot has no writer")]
        fn render_artifact_copilot_panics() {
            let _ = render_artifact(AgentId::Copilot, Capability::Consumer);
        }
    }

    // ---- splice ----

    mod splice {
        use super::*;

        fn block(inner: &str) -> String {
            format!("{DELIMITER_BEGIN}\n{inner}\n{DELIMITER_END}\n")
        }

        #[test]
        fn begin_marker_carries_the_current_version_stamp() {
            assert!(
                DELIMITER_BEGIN.contains(&format!("v{ARTIFACT_BODY_VERSION} ")),
                "DELIMITER_BEGIN must embed v{ARTIFACT_BODY_VERSION}, got {DELIMITER_BEGIN}"
            );
        }

        #[test]
        fn fresh_write_emits_versioned_marker() {
            match splice_managed_section(None, "BODY") {
                SpliceOutcome::FreshWrite(s) => {
                    assert!(s.starts_with(DELIMITER_BEGIN), "fresh write stamps version");
                    assert_eq!(s, block("BODY"));
                }
                other => panic!("expected FreshWrite, got {other:?}"),
            }
        }

        #[test]
        fn older_version_block_is_rewritten_in_place() {
            // An existing block stamped with an older version, surrounded by user
            // content, must be rewritten to the current marker — not duplicated.
            let existing = format!(
                "user-prefix\n<!-- repograph:begin v0 -->\nBODY\n{DELIMITER_END}\nuser-suffix\n"
            );
            match splice_managed_section(Some(&existing), "BODY") {
                SpliceOutcome::Replaced(s) => {
                    assert_eq!(s, format!("user-prefix\n{}user-suffix\n", block("BODY")));
                    assert_eq!(
                        s.matches("repograph:begin").count(),
                        1,
                        "no duplicate block"
                    );
                }
                other => panic!("expected Replaced for an older-version block, got {other:?}"),
            }
        }

        #[test]
        fn installed_version_parses_the_stamp() {
            let installed = block("BODY");
            assert_eq!(installed_version(&installed), Some(ARTIFACT_BODY_VERSION));
            assert_eq!(installed_version("# no managed block here\n"), None);
            assert_eq!(
                installed_version("<!-- repograph:begin v7 -->\nx\n<!-- repograph:end -->\n"),
                Some(7)
            );
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
            let r = install_one(AgentId::AgentsMd, Capability::Consumer, &path, false);
            match r {
                ArtifactResult::Written { path: p, .. } => assert_eq!(p, path),
                other => panic!("expected Written, got {other:?}"),
            }
            assert_eq!(
                read(&path),
                render_artifact(AgentId::AgentsMd, Capability::Consumer)
            );
        }

        #[test]
        fn re_run_with_identical_body_returns_unchanged() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("AGENTS.md");
            let _ = install_one(AgentId::AgentsMd, Capability::Consumer, &path, false);
            let first = read(&path);
            let r = install_one(AgentId::AgentsMd, Capability::Consumer, &path, false);
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
            let _ = install_one(AgentId::AgentsMd, Capability::Consumer, &path, false);
            let first = read(&path);
            let r = install_one(AgentId::AgentsMd, Capability::Consumer, &path, true);
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
            let r = install_one(AgentId::AgentsMd, Capability::Consumer, &path, true);
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
            let r = install_one(AgentId::ClaudeCode, Capability::Consumer, &path, false);
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
            let _ = install_one(AgentId::ClaudeCode, Capability::Consumer, &path, false);
            let first = read(&path);
            let r = install_one(AgentId::ClaudeCode, Capability::Consumer, &path, false);
            assert!(matches!(r, ArtifactResult::Unchanged { .. }));
            assert_eq!(read(&path), first);
        }

        #[test]
        fn non_force_preserves_user_content_around_block() {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("AGENTS.md");
            fs_err::write(&path, "# My project\n\nCustom prose.\n").unwrap();
            let r = install_one(AgentId::AgentsMd, Capability::Consumer, &path, false);
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
        fn emits_per_capability_in_selection_then_capability_order() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            let agents = vec![AgentId::AgentsMd, AgentId::ClaudeCode];
            let results = install_artifacts(&agents, Scope::User, &home, &cwd, false);
            // Flat-file AgentsMd → 1 combined artifact; wholly-owned ClaudeCode
            // → 2 SKILL.md (Consumer then Setup) + 1 always-loaded CLAUDE.md
            // pointer. Selection order is preserved.
            assert_eq!(results.len(), 4);
            assert_eq!(results[0].agent(), AgentId::AgentsMd);
            assert_eq!(results[0].capability(), Some(Capability::Consumer));
            assert_eq!(results[1].agent(), AgentId::ClaudeCode);
            assert_eq!(results[1].capability(), Some(Capability::Consumer));
            assert_eq!(results[2].agent(), AgentId::ClaudeCode);
            assert_eq!(results[2].capability(), Some(Capability::Setup));
            // The CLAUDE.md pointer follows the two skills, tagged Consumer.
            assert_eq!(results[3].agent(), AgentId::ClaudeCode);
            assert_eq!(results[3].capability(), Some(Capability::Consumer));
            let pointer_path = home.join(".claude/CLAUDE.md");
            assert!(pointer_path.exists(), "always-loaded pointer written");
        }

        #[test]
        fn wholly_owned_agent_writes_a_discrete_setup_file() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            let results =
                install_artifacts(&[AgentId::ClaudeCode], Scope::User, &home, &cwd, false);
            // 2 SKILL.md artifacts + 1 always-loaded CLAUDE.md pointer.
            assert_eq!(results.len(), 3);
            // The setup skill lands at its own discrete path.
            let setup_path = home.join(".claude/skills/repograph-setup/SKILL.md");
            assert!(setup_path.exists(), "setup SKILL.md should be written");
            let body = fs_err::read_to_string(&setup_path).unwrap();
            assert!(
                body.starts_with("---\nname: repograph-setup\n"),
                "setup artifact carries its own frontmatter, got:\n{body}"
            );
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
                // AgentsMd → 1 (Failed); ClaudeCode → 2 SKILL.md + 1 pointer.
                assert_eq!(results.len(), 4);
                assert!(matches!(results[0], ArtifactResult::Failed { .. }));
                assert!(matches!(
                    results[1],
                    ArtifactResult::Written { .. } | ArtifactResult::Unchanged { .. }
                ));
                assert!(matches!(
                    results[2],
                    ArtifactResult::Written { .. } | ArtifactResult::Unchanged { .. }
                ));
                assert!(matches!(
                    results[3],
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
            // Copilot → 1 Skipped; AgentsMd → 1 Written; ClaudeCode → 2 SKILL.md
            // + 1 CLAUDE.md pointer, all Written.
            assert_eq!(results.len(), 5);
            assert!(matches!(results[0], ArtifactResult::Skipped { .. }));
            assert!(matches!(results[1], ArtifactResult::Written { .. }));
            assert!(matches!(results[2], ArtifactResult::Written { .. }));
            assert!(matches!(results[3], ArtifactResult::Written { .. }));
            assert!(matches!(results[4], ArtifactResult::Written { .. }));
        }
    }

    // ---- refresh_installed_artifacts (doctor --fix engine) ----

    mod refresh {
        use super::*;

        /// Read a file, panicking on failure (test-only).
        fn read(p: &Path) -> String {
            fs_err::read_to_string(p).unwrap()
        }

        #[test]
        fn rewrites_a_stale_skill_block_in_place() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            // Plant an old-version SKILL.md at the user-scope consumer path.
            let p = resolve_path(
                AgentId::ClaudeCode,
                Capability::Consumer,
                Scope::User,
                &home,
                &cwd,
            );
            fs_err::create_dir_all(p.parent().unwrap()).unwrap();
            fs_err::write(
                &p,
                "---\nname: repograph\n---\n\n<!-- repograph:begin v0 -->\nOLD\n<!-- repograph:end -->\n",
            )
            .unwrap();

            let results = refresh_installed_artifacts(&[AgentId::ClaudeCode], &home, &cwd);

            assert!(
                results
                    .iter()
                    .any(|r| matches!(r, ArtifactResult::Written { .. })),
                "stale skill is rewritten"
            );
            let after = read(&p);
            assert!(
                after.contains(DELIMITER_BEGIN) && !after.contains("v0"),
                "block is brought to the current version, got:\n{after}"
            );
        }

        #[test]
        fn splices_pointer_into_existing_block_less_claude_md() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            // A project CLAUDE.md exists with only user prose — no repograph block.
            let claude_md = cwd.join("CLAUDE.md");
            fs_err::write(&claude_md, "# House rules\n\nBe nice.\n").unwrap();

            let results = refresh_installed_artifacts(&[AgentId::ClaudeCode], &home, &cwd);

            assert!(
                results
                    .iter()
                    .any(|r| matches!(r, ArtifactResult::Written { .. })),
                "pointer is spliced into the existing file"
            );
            let body = read(&claude_md);
            assert!(
                body.starts_with("# House rules\n\nBe nice.\n"),
                "user prose preserved, got:\n{body}"
            );
            assert!(
                body.contains(DELIMITER_BEGIN_PREFIX),
                "managed block added, got:\n{body}"
            );
        }

        #[test]
        fn never_creates_a_missing_artifact() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            // Nothing installed anywhere.
            let results = refresh_installed_artifacts(&[AgentId::ClaudeCode], &home, &cwd);
            assert!(results.is_empty(), "no existing files → nothing touched");
            assert!(
                !cwd.join("CLAUDE.md").exists(),
                "must not create a CLAUDE.md from scratch (scope is init's call)"
            );
            assert!(
                !home.join(".claude/skills/repograph/SKILL.md").exists(),
                "must not create a SKILL.md from scratch"
            );
        }

        #[test]
        fn current_artifact_is_left_unchanged() {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("proj");
            fs_err::create_dir_all(&home).unwrap();
            fs_err::create_dir_all(&cwd).unwrap();
            // Install everything current first.
            let _ = install_artifacts(&[AgentId::ClaudeCode], Scope::User, &home, &cwd, false);

            let results = refresh_installed_artifacts(&[AgentId::ClaudeCode], &home, &cwd);
            assert!(
                !results.is_empty()
                    && results
                        .iter()
                        .all(|r| matches!(r, ArtifactResult::Unchanged { .. })),
                "already-current artifacts report Unchanged, not Written"
            );
        }
    }
}
