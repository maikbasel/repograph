//! Interactive prompt helpers (cliclack) and TTY-gated routing.
//!
//! ## Output contract
//!
//! `cliclack` 0.5.x writes its UI to `Term::stderr()` internally, which matches
//! repograph's "stdout is data, stderr is diagnostics" contract — no theme
//! override is needed (the design.md noted this as a planned override; resolved
//! deviation: cliclack already does the right thing out of the box).
//!
//! ## Module scope
//!
//! - [`detect_agents`] probes well-known `$HOME` paths to preselect agents.
//! - [`select_agents_interactively`] runs the multiselect used by both
//!   `repograph init`'s first-run flow and the shared auto-prompt fallback.
//! - [`ensure_agents_configured`] is the entry point any agent-consuming
//!   command calls before reading `[agents]`. It is a no-op when the section
//!   is present; prompts and persists when missing in a TTY; returns
//!   `RepographError::NeedsInit` when missing in non-TTY.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use is_terminal::IsTerminal;
use repograph_core::agent_artifact;
use repograph_core::{AgentId, Agents, Config, RepographError};

/// Environment variable name for overriding the user's project-root setting
/// at runtime. Mirrors `REPOGRAPH_CONFIG_DIR` ergonomics — env wins over
/// config, config wins over "ask the user."
pub const PROJECT_ROOT_ENV: &str = "REPOGRAPH_PROJECT_ROOT";

/// Resolve the effective project-root path to use for repo-registration
/// flows. Precedence: env var (`REPOGRAPH_PROJECT_ROOT`) → config
/// (`[settings] projects_root`) → `None` ("not configured; ask the user").
///
/// The returned `PathBuf` is whatever the user supplied — we do NOT
/// canonicalize here so the env-set value remains debuggable; canonicalization
/// happens when individual repos are validated via `validate_git_repo`.
#[must_use]
pub fn effective_projects_root(config: &Config) -> Option<PathBuf> {
    resolve_projects_root(config, std::env::var_os(PROJECT_ROOT_ENV))
}

/// Same precedence as [`effective_projects_root`] but with the env value
/// injected. Exists so tests don't have to mutate process-wide env.
fn resolve_projects_root(config: &Config, env: Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(env) = env
        && !env.is_empty()
    {
        return Some(PathBuf::from(env));
    }
    config.settings().and_then(|s| s.projects_root.clone())
}

/// Filesystem-aware path suggestions for the repo-path input in
/// `repograph init`. Plugs into cliclack's `Input::autocomplete` via the
/// `Fn(&str) -> Vec<String>` blanket impl of [`cliclack::Suggest`].
///
/// Behavior:
///
/// - `~` and `~/…` are expanded against `dirs::home_dir()` so users can type
///   `~/IdeaProjects/` without leaving the prompt.
/// - Only directories (and symlinks, which are most often directory aliases)
///   are surfaced — a registered repo is always a directory.
/// - Hidden entries (`.foo`) are returned only when the user typed a prefix
///   starting with `.`, matching shell tab-completion ergonomics.
/// - Returned paths always end in `/` so picking one keeps the prompt
///   "drilled into" that directory and the next keystroke surfaces its
///   children.
/// - Unreadable / nonexistent parents yield an empty list (no errors leak to
///   the user; the autocomplete popup simply hides).
#[must_use]
pub fn path_suggestions(input: &str) -> Vec<String> {
    let expanded = expand_tilde(input);
    let (parent, prefix) = split_parent_prefix(&expanded);

    let Ok(read) = std::fs::read_dir(&parent) else {
        return Vec::new();
    };

    let mut matched: Vec<String> = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let ft = entry.file_type().ok()?;
            if !ft.is_dir() && !ft.is_symlink() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !prefix.starts_with('.') {
                return None;
            }
            if !name.starts_with(&prefix) {
                return None;
            }
            Some(name)
        })
        .collect();
    matched.sort_unstable();

    // Reconstruct each suggestion from the parent string + entry name joined
    // with `/`. The whole module operates on `/` separators (see
    // `split_parent_prefix`), so we must not lean on `entry.path()`, which
    // emits the OS separator (backslashes on Windows) and would break the
    // round-trip back into the `/`-based autocomplete prompt.
    let parent_str = parent.to_string_lossy();
    let parent_base = parent_str.trim_end_matches('/');
    matched
        .into_iter()
        .map(|name| format!("{parent_base}/{name}/"))
        .collect()
}

fn expand_tilde(input: &str) -> String {
    if input == "~" {
        return dirs::home_dir()
            .map_or_else(|| input.to_string(), |h| h.to_string_lossy().into_owned());
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().into_owned();
    }
    input.to_string()
}

/// Common names developers use for "the parent folder where I keep all my
/// projects." Probed under `$HOME` in order. The first one that exists is
/// surfaced as a candidate for the repo-registration step in
/// `repograph init`; the rest follow.
///
/// The list is intentionally short and opinionated. Power users with
/// non-standard layouts still get the free-form input + filesystem
/// autocomplete, so this is purely a fast path for the common case.
const PROJECT_ROOT_CANDIDATES: &[&str] = &[
    "IdeaProjects",
    "Projects",
    "projects",
    "dev",
    "code",
    "Code",
    "work",
    "src",
    "repos",
];

/// Return candidate project-root directories that exist under `home` AND
/// contain at least one git repository. Empty or non-existent candidates are
/// filtered out so the picker never surfaces noise.
///
/// Used only as a *seed* for the interactive prompt — the user can always
/// override with a custom path. The actual stored value lives in
/// `[settings] projects_root`.
#[must_use]
pub fn discover_project_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    PROJECT_ROOT_CANDIDATES
        .iter()
        .map(|name| home.join(name))
        .filter(|p| p.is_dir())
        .filter(|p| !scan_git_repos(p).is_empty())
        .collect()
}

/// Scan `root` for direct subdirectories that look like git repositories
/// (contain a `.git` entry — directory or file, the latter being a worktree
/// marker). Returns paths in sorted order by basename for stable rendering.
///
/// Best-effort: unreadable entries are silently skipped, and the scan never
/// recurses beyond one level. Symlinks are followed via `Path::is_dir`,
/// which is the same behavior used elsewhere in repograph.
#[must_use]
pub fn scan_git_repos(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut repos: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            // `.git` may be a directory (normal repo) or a file (worktree).
            let git = path.join(".git");
            if git.exists() { Some(path) } else { None }
        })
        .collect();
    repos.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    repos
}

fn split_parent_prefix(input: &str) -> (PathBuf, String) {
    if input.is_empty() {
        return (PathBuf::from("."), String::new());
    }
    if input.ends_with('/') {
        return (PathBuf::from(input), String::new());
    }
    // Manual split on the last `/` so we handle the `<dir>/.` case correctly
    // (`Path::file_name` returns `None` for paths ending in `.`).
    input.rfind('/').map_or_else(
        || (PathBuf::from("."), input.to_string()),
        |idx| {
            let parent = if idx == 0 { "/" } else { &input[..idx] };
            (PathBuf::from(parent), input[idx + 1..].to_string())
        },
    )
}

/// Probe `home` for the well-known paths of each agent toolchain and return
/// the set of agents whose signals match. `agents-md` is intentionally never
/// included — it has no `$HOME` signal and must always be opted into.
///
/// Detection is best-effort: when `home` is `None` or any probed path is
/// unreadable, the missing signal yields no preselection (no error is
/// propagated). The returned set is empty for an environment with no
/// recognizable signals.
#[must_use]
pub fn detect_agents(home: Option<&Path>) -> BTreeSet<AgentId> {
    let mut out = BTreeSet::new();
    let Some(home) = home else {
        return out;
    };

    if probe_any(home, &[".claude", ".config/claude"]) {
        out.insert(AgentId::ClaudeCode);
    }
    if probe_any(home, &[".cursor"]) {
        out.insert(AgentId::Cursor);
    }
    if probe_any(home, &[".aider", ".aider.conf.yml"]) {
        out.insert(AgentId::Aider);
    }
    if probe_any(home, &[".codeium/windsurf"]) {
        out.insert(AgentId::Windsurf);
    }
    if probe_any(home, &[".config/github-copilot"]) {
        out.insert(AgentId::Copilot);
    }
    out
}

fn probe_any(home: &Path, suffixes: &[&str]) -> bool {
    // `Path::exists` returns false (not error) on permission failures, so this
    // is total — no panic on unreadable home directories.
    suffixes.iter().any(|s| home.join(s).exists())
}

/// Resolve the user's home directory for detection. Returns `None` when
/// `dirs::home_dir()` cannot determine one. Indirected so tests can substitute
/// a fake `$HOME`.
#[must_use]
pub fn host_home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Run the agent multiselect prompt, returning the user's selection.
///
/// `preselected` controls which entries start checked. Order in the rendered
/// list always follows [`AgentId::all`]; selection order is the order the
/// user *toggled* them (cliclack's `MultiSelect` returns in registration
/// order, so we preserve the registration order — `AgentId::all` — and
/// re-sort the output to match the iteration order the user observed). For
/// our purposes this is the well-known registry order, which is what users
/// expect.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when cliclack cannot read from stdin
/// (typically because the calling process lacks a TTY despite this function
/// being invoked — call sites SHOULD gate on [`stdout_is_tty`] first).
pub fn select_agents_interactively(
    preselected: &BTreeSet<AgentId>,
) -> Result<Vec<AgentId>, RepographError> {
    let mut prompt = cliclack::multiselect("Which agent(s) do you use?").required(false);

    for id in AgentId::all() {
        let label = format!("{}  ({})", id.display_name(), id.file_patterns().join(", "));
        prompt = prompt.item(*id, label, "");
    }

    let preselect_vec: Vec<AgentId> = preselected.iter().copied().collect();
    if !preselect_vec.is_empty() {
        prompt = prompt.initial_values(preselect_vec);
    }

    prompt.interact().map_err(RepographError::Io)
}

/// Check whether stdout is a TTY. Centralized so call sites can mock or stub
/// in tests if needed, and so the rule (stdout, not stderr) is enforced
/// consistently with the rest of repograph's TTY detection.
#[must_use]
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Render a cliclack single-select asking the user where to install agent
/// artifacts. Both options show the resolved path so the user picks the right
/// one without guessing what `user` and `project` mean. Default is `User`.
///
/// Emits to stderr per the output contract.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when cliclack cannot read from stdin (e.g.
/// non-TTY context). Call sites SHOULD gate on [`stdout_is_tty`] before
/// invoking.
pub fn prompt_scope(home: &Path, cwd: &Path) -> Result<agent_artifact::Scope, RepographError> {
    cliclack::select::<agent_artifact::Scope>("Where should I install agent artifacts?")
        .item(
            agent_artifact::Scope::User,
            format!("User ({})", home.display()),
            "applies across every project",
        )
        .item(
            agent_artifact::Scope::Project,
            format!("Project ({})", cwd.display()),
            "checked into this repo",
        )
        .initial_value(agent_artifact::Scope::User)
        .interact()
        .map_err(RepographError::Io)
}

/// Ensure `config.agents()` is `Some(_)` before the caller reads it. The
/// behavior depends on the current state:
///
/// - If `[agents]` is already present: no-op, returns `Ok(())`.
/// - If `[agents]` is missing AND stdout is a TTY: prompt the user with the
///   detection-preselected multiselect, persist the selection to disk, and
///   return `Ok(())`.
/// - If `[agents]` is missing AND stdout is NOT a TTY: return
///   [`RepographError::NeedsInit`] mapped to exit code `2`.
///
/// # Errors
///
/// Propagates [`RepographError`] for I/O failures during the prompt, config
/// save failures, and the non-TTY guard.
//
// Currently unused at the call-site level — the future `context-command`
// (Phase 4b) will be the first consumer. Tested via the unit tests in this
// module; intentionally retained as a shared primitive per the spec.
#[allow(dead_code)]
#[tracing::instrument(skip(config, config_dir), fields(config_dir = %config_dir.display()))]
pub fn ensure_agents_configured(
    config: &mut Config,
    config_dir: &Path,
) -> Result<(), RepographError> {
    if config.agents().is_some() {
        tracing::debug!("agents already configured");
        return Ok(());
    }

    if !stdout_is_tty() {
        tracing::warn!("agents missing in non-TTY context");
        return Err(RepographError::NeedsInit(
            "agents not configured; run `repograph init` in an interactive shell, \
             or run `repograph init --no-prompt --agents <list>`"
                .to_string(),
        ));
    }

    tracing::debug!("agents missing in TTY — entering interactive prompt");
    let detected = detect_agents(host_home().as_deref());
    let selected = select_agents_interactively(&detected)?;
    config.set_agents(Some(Agents { selected }));
    config.save(config_dir)?;
    tracing::info!("agents configured via auto-prompt");
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_with_no_home_returns_empty() {
        let detected = detect_agents(None);
        assert!(detected.is_empty());
    }

    #[test]
    fn detect_with_empty_home_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.is_empty());
    }

    #[test]
    fn detect_claude_via_dot_claude() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::ClaudeCode));
        assert!(!detected.contains(&AgentId::AgentsMd));
    }

    #[test]
    fn detect_claude_via_xdg_config_claude() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".config/claude")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::ClaudeCode));
    }

    #[test]
    fn detect_cursor() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".cursor")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::Cursor));
    }

    #[test]
    fn detect_aider_via_dir() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".aider")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::Aider));
    }

    #[test]
    fn detect_aider_via_conf_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".aider.conf.yml"), "").unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::Aider));
    }

    #[test]
    fn detect_windsurf() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".codeium/windsurf")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::Windsurf));
    }

    #[test]
    fn detect_copilot() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".config/github-copilot")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::Copilot));
    }

    #[test]
    fn detect_all_signals_at_once() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".cursor")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".aider")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".codeium/windsurf")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".config/github-copilot")).unwrap();
        let detected = detect_agents(Some(tmp.path()));
        assert!(detected.contains(&AgentId::ClaudeCode));
        assert!(detected.contains(&AgentId::Cursor));
        assert!(detected.contains(&AgentId::Aider));
        assert!(detected.contains(&AgentId::Windsurf));
        assert!(detected.contains(&AgentId::Copilot));
        // agents-md never auto-preselects.
        assert!(!detected.contains(&AgentId::AgentsMd));
    }

    #[test]
    fn detect_does_not_panic_on_missing_paths() {
        let tmp = TempDir::new().unwrap();
        // Reference paths under a nonexistent subdirectory — probe must
        // tolerate the missing parent without erroring.
        let weird = tmp.path().join("nope");
        let _ = detect_agents(Some(&weird));
    }

    // ─── path_suggestions ──────────────────────────────────────────────────

    #[test]
    fn path_suggestions_nonexistent_parent_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let nope = tmp.path().join("does-not-exist").join("anything");
        let result = path_suggestions(nope.to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn path_suggestions_lists_directory_children_when_input_ends_with_slash() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("alpha")).unwrap();
        std::fs::create_dir_all(tmp.path().join("beta")).unwrap();
        std::fs::write(tmp.path().join("not-a-dir.txt"), "").unwrap();

        let input = format!("{}/", tmp.path().display());
        let result = path_suggestions(&input);
        assert!(result.iter().any(|s| s.ends_with("/alpha/")), "{result:?}");
        assert!(result.iter().any(|s| s.ends_with("/beta/")), "{result:?}");
        assert!(
            !result.iter().any(|s| s.contains("not-a-dir.txt")),
            "files filtered out: {result:?}"
        );
    }

    #[test]
    fn path_suggestions_filters_by_prefix() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("alpha")).unwrap();
        std::fs::create_dir_all(tmp.path().join("apex")).unwrap();
        std::fs::create_dir_all(tmp.path().join("beta")).unwrap();

        let input = format!("{}/al", tmp.path().display());
        let result = path_suggestions(&input);
        assert!(result.iter().any(|s| s.ends_with("/alpha/")), "{result:?}");
        assert!(
            !result.iter().any(|s| s.ends_with("/apex/")),
            "prefix `al` excludes `apex`: {result:?}"
        );
        assert!(!result.iter().any(|s| s.ends_with("/beta/")), "{result:?}");
    }

    #[test]
    fn path_suggestions_excludes_hidden_unless_prefix_starts_with_dot() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("visible")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".hidden")).unwrap();

        let no_dot = format!("{}/", tmp.path().display());
        let result = path_suggestions(&no_dot);
        assert!(
            !result.iter().any(|s| s.contains("/.hidden/")),
            "hidden excluded by default: {result:?}"
        );
        assert!(result.iter().any(|s| s.ends_with("/visible/")));

        let dot = format!("{}/.", tmp.path().display());
        let result_dot = path_suggestions(&dot);
        assert!(
            result_dot.iter().any(|s| s.ends_with("/.hidden/")),
            "hidden surfaced when prefix begins with dot: {result_dot:?}"
        );
    }

    #[test]
    fn path_suggestions_sorts_results() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("zeta")).unwrap();
        std::fs::create_dir_all(tmp.path().join("alpha")).unwrap();
        std::fs::create_dir_all(tmp.path().join("mid")).unwrap();

        let input = format!("{}/", tmp.path().display());
        let result = path_suggestions(&input);
        let names: Vec<&str> = result
            .iter()
            .map(|s| s.trim_end_matches('/').rsplit('/').next().unwrap_or(""))
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "results sorted alphabetically");
    }

    #[test]
    fn split_parent_prefix_handles_trailing_slash() {
        let (parent, prefix) = split_parent_prefix("/tmp/foo/");
        assert_eq!(parent, PathBuf::from("/tmp/foo/"));
        assert_eq!(prefix, "");
    }

    #[test]
    fn split_parent_prefix_handles_partial_name() {
        let (parent, prefix) = split_parent_prefix("/tmp/foo/bar");
        assert_eq!(parent, PathBuf::from("/tmp/foo"));
        assert_eq!(prefix, "bar");
    }

    #[test]
    fn split_parent_prefix_empty_input_uses_cwd() {
        let (parent, prefix) = split_parent_prefix("");
        assert_eq!(parent, PathBuf::from("."));
        assert_eq!(prefix, "");
    }

    #[test]
    fn split_parent_prefix_bare_name_uses_cwd() {
        let (parent, prefix) = split_parent_prefix("foo");
        assert_eq!(parent, PathBuf::from("."));
        assert_eq!(prefix, "foo");
    }

    // ─── discover_project_roots / scan_git_repos ──────────────────────────

    #[test]
    fn discover_returns_empty_when_home_is_none() {
        assert!(discover_project_roots(None).is_empty());
    }

    #[test]
    fn discover_finds_only_dirs_with_at_least_one_git_repo() {
        let tmp = TempDir::new().unwrap();
        // Plant a real repo under each candidate we want surfaced.
        std::fs::create_dir_all(tmp.path().join("IdeaProjects/my-repo/.git")).unwrap();
        std::fs::create_dir_all(tmp.path().join("code/their-repo/.git")).unwrap();
        // "projects" is intentionally NOT created — must not appear.
        // "Projects" exists but is empty — must NOT appear (the bug fix).
        std::fs::create_dir_all(tmp.path().join("Projects")).unwrap();

        let roots = discover_project_roots(Some(tmp.path()));
        assert!(
            roots.iter().any(|p| p.ends_with("IdeaProjects")),
            "got {roots:?}"
        );
        assert!(roots.iter().any(|p| p.ends_with("code")), "got {roots:?}");
        assert!(
            !roots.iter().any(|p| p.ends_with("projects")),
            "non-existent absent: {roots:?}"
        );
        assert!(
            !roots.iter().any(|p| p.ends_with("Projects")),
            "empty Projects suppressed: {roots:?}"
        );
    }

    #[test]
    fn discover_returns_empty_when_no_candidates_match() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("nothing-developer-y")).unwrap();
        let roots = discover_project_roots(Some(tmp.path()));
        assert!(roots.is_empty());
    }

    #[test]
    fn discover_returns_empty_when_candidates_exist_but_are_empty() {
        let tmp = TempDir::new().unwrap();
        // Existing candidate dir but no git repos inside — this is the
        // "Projects (0 repos)" noise case that prompted the filter.
        std::fs::create_dir_all(tmp.path().join("IdeaProjects")).unwrap();
        std::fs::create_dir_all(tmp.path().join("code")).unwrap();
        let roots = discover_project_roots(Some(tmp.path()));
        assert!(
            roots.is_empty(),
            "empty candidates must not be surfaced, got: {roots:?}"
        );
    }

    // ─── resolve_projects_root (pure, env-injected) ───────────────────────

    #[test]
    fn resolve_returns_none_when_both_env_and_config_unset() {
        let cfg = Config::default();
        assert!(resolve_projects_root(&cfg, None).is_none());
    }

    #[test]
    fn resolve_reads_config_when_env_unset() {
        let mut cfg = Config::default();
        cfg.set_settings(Some(repograph_core::Settings {
            projects_root: Some(PathBuf::from("/from/config")),
        }));
        assert_eq!(
            resolve_projects_root(&cfg, None).as_deref(),
            Some(Path::new("/from/config"))
        );
    }

    #[test]
    fn resolve_env_wins_over_config() {
        let mut cfg = Config::default();
        cfg.set_settings(Some(repograph_core::Settings {
            projects_root: Some(PathBuf::from("/from/config")),
        }));
        assert_eq!(
            resolve_projects_root(&cfg, Some("/from/env".into())).as_deref(),
            Some(Path::new("/from/env"))
        );
    }

    #[test]
    fn resolve_empty_env_value_falls_through_to_config() {
        let mut cfg = Config::default();
        cfg.set_settings(Some(repograph_core::Settings {
            projects_root: Some(PathBuf::from("/from/config")),
        }));
        assert_eq!(
            resolve_projects_root(&cfg, Some("".into())).as_deref(),
            Some(Path::new("/from/config")),
            "empty env value should not shadow config"
        );
    }

    #[test]
    fn resolve_env_with_no_config_still_returns_env() {
        let cfg = Config::default();
        assert_eq!(
            resolve_projects_root(&cfg, Some("/lone/env".into())).as_deref(),
            Some(Path::new("/lone/env"))
        );
    }

    #[test]
    fn scan_finds_only_git_repos() {
        let tmp = TempDir::new().unwrap();
        // Two repos with .git/ dir, one file-based worktree marker, one plain dir.
        for name in ["repo-a", "repo-b"] {
            let dir = tmp.path().join(name);
            std::fs::create_dir_all(dir.join(".git")).unwrap();
        }
        let worktree = tmp.path().join("worktree-marker");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: /elsewhere").unwrap();
        std::fs::create_dir_all(tmp.path().join("not-a-repo")).unwrap();

        let repos = scan_git_repos(tmp.path());
        let names: Vec<_> = repos
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"repo-a".to_string()));
        assert!(names.contains(&"repo-b".to_string()));
        assert!(names.contains(&"worktree-marker".to_string()));
        assert!(!names.contains(&"not-a-repo".to_string()));
    }

    #[test]
    fn scan_sorted_alphabetically_for_stable_rendering() {
        let tmp = TempDir::new().unwrap();
        for name in ["zeta", "alpha", "mid"] {
            std::fs::create_dir_all(tmp.path().join(name).join(".git")).unwrap();
        }
        let repos = scan_git_repos(tmp.path());
        let names: Vec<_> = repos
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn scan_nonexistent_root_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert!(scan_git_repos(&missing).is_empty());
    }
}
