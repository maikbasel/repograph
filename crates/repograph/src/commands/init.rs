//! `repograph init` — interactive setup command.
//!
//! Two modes:
//!
//! - **Non-interactive** (`--no-prompt --agents <list>`): validates the list,
//!   writes `[agents] selected = [...]` to config, returns. No cliclack UI.
//!   Required for CI / automation / non-TTY contexts.
//! - **Interactive** (default in a TTY): routes to either the first-run flow
//!   (no `[agents]` section yet) or the settings-panel flow (already
//!   configured). All UI via cliclack; stdout is never touched.
//!
//! ## Manual validation script
//!
//! Manual TTY scenarios — first run, full first run with repo + workspace,
//! settings panel, reset, etc. — are documented in
//! `openspec/changes/init-command/design.md` under "Manual Validation Script"
//! and must be walked before archiving the change.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::Parser;
use repograph_core::agent_artifact::{
    self, ArtifactResult, has_artifact_writer, install_artifacts, scope_is_meaningful,
};
use repograph_core::{
    AgentId, Agents, Config, Repo, RepographError, validate_git_repo, validate_workspace_name,
};

use crate::prompt::{
    PROJECT_ROOT_ENV, detect_agents, discover_project_roots, effective_projects_root, host_home,
    path_suggestions, prompt_scope, scan_git_repos, select_agents_interactively, stdout_is_tty,
};

#[derive(Debug, Parser)]
pub struct Args {
    /// Comma-separated agent IDs to configure. Required with `--no-prompt`;
    /// optional in interactive mode (preselects but lets the user adjust).
    /// Valid IDs: `claude-code`, `agents-md`, `cursor`, `aider`, `windsurf`,
    /// `copilot`.
    #[arg(long, value_name = "LIST")]
    pub agents: Option<String>,

    /// Skip the interactive flow entirely. Requires `--agents`. Useful for CI,
    /// dotfile bootstrapping, and non-TTY contexts.
    #[arg(long, requires = "agents")]
    pub no_prompt: bool,

    /// Where to install agent artifacts. Defaults to `user` when omitted;
    /// required under `--no-prompt` when any selected agent has a meaningful
    /// scope choice (i.e. its user and project paths differ — today that's
    /// `claude-code` and `windsurf`). Project-only agents silently use the
    /// project path regardless of this flag.
    #[arg(long, value_parser = parse_scope, value_name = "SCOPE")]
    pub scope: Option<agent_artifact::Scope>,

    /// Overwrite existing artifacts even outside the managed delimiter block.
    /// Without this flag, repograph rewrites only the delimited region of
    /// pre-existing files (preserving user content); with it, the file is
    /// replaced fresh.
    #[arg(long)]
    pub force: bool,
}

/// Parse the `--scope` CLI value. Mirrors `Scope`'s serde lowercase rendering
/// so the user-facing surface is the same on the wire and on the command line.
fn parse_scope(s: &str) -> Result<agent_artifact::Scope, String> {
    match s {
        "user" => Ok(agent_artifact::Scope::User),
        "project" => Ok(agent_artifact::Scope::Project),
        other => Err(format!(
            "invalid scope '{other}', expected `user` or `project`"
        )),
    }
}

/// Entry point dispatched from `main.rs`.
///
/// # Errors
///
/// Propagates [`RepographError`] for the documented failure modes. See the
/// `init-command` spec for the full table of scenarios.
#[tracing::instrument(skip(args, config_dir), fields(
    no_prompt = args.no_prompt,
    agents_flag = args.agents.as_deref().unwrap_or("<none>"),
    scope = ?args.scope,
    force = args.force,
    config_dir = %config_dir.display(),
))]
pub fn run(args: &Args, config_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("init: start");

    let mut config = Config::load(config_dir)?;

    if args.no_prompt {
        run_non_interactive(args, &mut config, config_dir)?;
        tracing::info!("init: completed (non-interactive)");
        return Ok(());
    }

    if !stdout_is_tty() {
        tracing::warn!("init: non-TTY without --no-prompt");
        return Err(RepographError::NeedsInit(
            "stdout is not a TTY; pass `--no-prompt --agents <list>` to run \
             `repograph init` non-interactively, or invoke it in an interactive shell"
                .to_string(),
        ));
    }

    if config.agents().is_some() {
        run_settings_panel(args, &mut config, config_dir)?;
    } else {
        run_first_run(args, &mut config, config_dir)?;
    }
    tracing::info!("init: completed (interactive)");
    Ok(())
}

/// Does the selection contain at least one agent for which `Scope::User` and
/// `Scope::Project` resolve to different paths? If yes, `--scope` must be
/// explicit under `--no-prompt`.
fn requires_scope(selected: &[AgentId]) -> bool {
    selected
        .iter()
        .any(|&a| has_artifact_writer(a) && scope_is_meaningful(a))
}

/// Comma-joined `as_str()` form of the agents in `selected` for which
/// `scope_is_meaningful` is true. Used to name the offending agents in the
/// `--no-prompt`/`--scope` validation error.
fn scope_bearing_names(selected: &[AgentId]) -> String {
    selected
        .iter()
        .filter(|&&a| has_artifact_writer(a) && scope_is_meaningful(a))
        .map(AgentId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Resolve the install scope for an interactive run. If the user passed
/// `--scope` on the command line, use that. Otherwise, if at least one
/// selected agent has a meaningful scope choice, prompt; if not, default to
/// `User` (which falls through to project for project-only agents anyway).
fn resolve_scope_interactive(
    args: &Args,
    selected: &[AgentId],
) -> Result<agent_artifact::Scope, RepographError> {
    if let Some(s) = args.scope {
        return Ok(s);
    }
    if !requires_scope(selected) {
        return Ok(agent_artifact::Scope::User);
    }
    let home = host_home().unwrap_or_else(|| PathBuf::from("~"));
    let cwd = std::env::current_dir().map_err(RepographError::Io)?;
    prompt_scope(&home, &cwd)
}

/// Install per-agent artifacts for `selected` and log per-result outcomes on
/// stderr. No-op if no agent in the selection has a writer.
///
/// Returns `Err` only for failures that prevent the install from running at
/// all (e.g. `current_dir()` failed, or `--scope user` was chosen but the
/// host has no home directory). Per-agent install failures are captured as
/// [`ArtifactResult::Failed`] and surfaced via `warn!`; they do NOT abort the
/// command's exit code.
fn run_install(
    selected: &[AgentId],
    scope: agent_artifact::Scope,
    force: bool,
) -> Result<(), RepographError> {
    if !selected.iter().any(|&a| has_artifact_writer(a)) {
        return Ok(());
    }
    let cwd = std::env::current_dir().map_err(RepographError::Io)?;
    let home = match host_home() {
        Some(h) => h,
        None if scope == agent_artifact::Scope::Project => cwd.clone(),
        None => {
            return Err(RepographError::UsageError(
                "could not determine home directory for `--scope user`; \
                 pass `--scope project` or set `HOME` in the environment"
                    .into(),
            ));
        }
    };
    let results = install_artifacts(selected, scope, &home, &cwd, force);
    log_install_results(&results);
    Ok(())
}

/// Emit one `tracing` line per install result on stderr per the logging
/// contract. `Failed` results use `warn!`; everything else `info!`.
fn log_install_results(results: &[ArtifactResult]) {
    for r in results {
        match r {
            ArtifactResult::Written { agent, path } => {
                tracing::info!(
                    agent = agent.as_str(),
                    path = %path.display(),
                    "artifact written",
                );
            }
            ArtifactResult::Unchanged { agent, path } => {
                tracing::info!(
                    agent = agent.as_str(),
                    path = %path.display(),
                    "artifact unchanged",
                );
            }
            ArtifactResult::Skipped { agent, reason } => {
                tracing::info!(
                    agent = agent.as_str(),
                    reason = *reason,
                    "artifact skipped",
                );
            }
            ArtifactResult::Failed { agent, error } => {
                tracing::warn!(
                    agent = agent.as_str(),
                    err = ?error,
                    "artifact failed",
                );
            }
        }
    }
}

fn run_non_interactive(
    args: &Args,
    config: &mut Config,
    config_dir: &Path,
) -> Result<(), RepographError> {
    // clap's `requires = "agents"` on `--no-prompt` guarantees `args.agents`
    // is `Some` at this point. The empty-string case is treated as "no
    // selection" (a valid configured-but-empty state).
    let list = args.agents.as_deref().unwrap_or("");
    let selected = parse_agent_list(list)?;

    if requires_scope(&selected) && args.scope.is_none() {
        return Err(RepographError::UsageError(format!(
            "--scope must be explicit under --no-prompt when selected agents include \
             {names}; pass `--scope user` or `--scope project`",
            names = scope_bearing_names(&selected),
        )));
    }

    config.set_agents(Some(Agents {
        selected: selected.clone(),
    }));
    config.save(config_dir)?;

    let scope = args.scope.unwrap_or(agent_artifact::Scope::User);
    run_install(&selected, scope, args.force)?;
    Ok(())
}

/// Parse the comma-separated `--agents` string into a `Vec<AgentId>`.
/// Empty input yields an empty vector (a valid configured-but-empty state).
/// Whitespace around entries is trimmed. Unknown IDs propagate as
/// `RepographError::InvalidName` (exit `2`).
fn parse_agent_list(list: &str) -> Result<Vec<AgentId>, RepographError> {
    if list.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in list.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let id = AgentId::parse(token)?;
        if seen.insert(id) {
            out.push(id);
        }
    }
    Ok(out)
}

fn run_first_run(
    args: &Args,
    config: &mut Config,
    config_dir: &Path,
) -> Result<(), RepographError> {
    cliclack::intro("repograph init").map_err(RepographError::Io)?;
    cliclack::note(
        "Welcome",
        "Let's get repograph set up for your agent toolchain.",
    )
    .map_err(RepographError::Io)?;

    // Detection seed + optional --agents preselect override.
    let mut preselected = detect_agents(host_home().as_deref());
    if let Some(list) = args.agents.as_deref() {
        let parsed = parse_agent_list(list)?;
        preselected.extend(parsed);
    }

    let selected = select_agents_interactively(&preselected)?;
    // Resolve install scope before the existing project-root flow so the user
    // makes all the agent-related decisions up front.
    let scope = resolve_scope_interactive(args, &selected)?;

    config.set_agents(Some(Agents {
        selected: selected.clone(),
    }));
    config.save(config_dir)?;

    run_install(&selected, scope, args.force)?;

    // One-time project-root setup. Stored persistently so future repo
    // registrations and the future `context` command can use it.
    pick_projects_root_step(config, config_dir)?;

    // Optional repo registration + bulk workspace assignment.
    let registered = maybe_register_repos(config, config_dir)?;
    maybe_assign_repos_to_workspaces(config, config_dir, &registered)?;

    finish_outro(config, &selected)
}

/// First-run step asking "Where do you keep your projects?" and persisting
/// the answer to `[settings] projects_root`. Skip-able. No-ops when the
/// effective value is already known (env var or pre-existing config).
fn pick_projects_root_step(config: &mut Config, config_dir: &Path) -> Result<(), RepographError> {
    if effective_projects_root(config).is_some() {
        return Ok(());
    }
    ask_and_store_projects_root(config, config_dir)
}

/// Render the projects-root prompt unconditionally and persist the result.
/// Used by the first-run flow (gated by [`pick_projects_root_step`]) and by
/// the settings-panel "Change project root" action (which always wants to
/// ask, regardless of current state).
///
/// Detected candidates (existing directories under `$HOME` that contain at
/// least one git repo) are surfaced as primary options; the user can also
/// type a custom path or skip. The answer is written to
/// `[settings] projects_root`.
enum ProjectsRootChoice {
    Use(PathBuf),
    Custom,
    Skip,
}

fn ask_and_store_projects_root(
    config: &mut Config,
    config_dir: &Path,
) -> Result<(), RepographError> {
    let detected = discover_project_roots(host_home().as_deref());

    let mut select = cliclack::select::<usize>("Where do you keep your projects?");
    let mut choices: Vec<ProjectsRootChoice> = Vec::new();
    for root in &detected {
        let count = scan_git_repos(root).len();
        let label = format!("{}  ({count} repos)", root.display());
        choices.push(ProjectsRootChoice::Use(root.clone()));
        select = select.item(choices.len() - 1, label, "");
    }
    choices.push(ProjectsRootChoice::Custom);
    select = select.item(choices.len() - 1, "Enter a custom path...", "");
    choices.push(ProjectsRootChoice::Skip);
    select = select.item(
        choices.len() - 1,
        "Skip — I'll set this later",
        "change anytime from `repograph init`",
    );

    let idx = select.interact().map_err(RepographError::Io)?;
    let new_root: Option<PathBuf> = match &choices[idx] {
        ProjectsRootChoice::Use(p) => Some(p.clone()),
        ProjectsRootChoice::Custom => {
            let raw: String = cliclack::input("Project root path")
                .placeholder("/home/you/code  (Tab to autocomplete)")
                .autocomplete(path_suggestions)
                .interact()
                .map_err(RepographError::Io)?;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                let path = PathBuf::from(trimmed);
                if !path.is_dir() {
                    cliclack::log::warning(format!(
                        "{} does not currently exist — stored anyway, will be \
                         checked next time it's used",
                        path.display()
                    ))
                    .map_err(RepographError::Io)?;
                }
                Some(path)
            }
        }
        ProjectsRootChoice::Skip => None,
    };

    let mut settings = config.settings().cloned().unwrap_or_default();
    settings.projects_root = new_root;
    // Persist the section even when the chosen value is None, so the
    // section's presence records "the user has answered this question."
    config.set_settings(Some(settings));
    config.save(config_dir)?;
    Ok(())
}

fn run_settings_panel(
    args: &Args,
    config: &mut Config,
    config_dir: &Path,
) -> Result<(), RepographError> {
    cliclack::intro("repograph init").map_err(RepographError::Io)?;
    let current = config
        .agents()
        .map(|a| a.selected.clone())
        .unwrap_or_default();
    let root_label = effective_projects_root(config)
        .map_or_else(|| "(not set)".to_string(), |p| p.display().to_string());
    let summary = format!(
        "agents:        {}\nprojects root: {}\nrepos:         {}\nworkspaces:    {}",
        if current.is_empty() {
            "(none)".to_string()
        } else {
            current
                .iter()
                .map(AgentId::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        },
        root_label,
        config.repos().len(),
        config.workspaces().len(),
    );
    cliclack::note("Current configuration", summary).map_err(RepographError::Io)?;

    loop {
        let action: SettingsAction = cliclack::select("What would you like to do?")
            .item(SettingsAction::UpdateAgents, "Update agent selection", "")
            .item(SettingsAction::ChangeProjectRoot, "Change project root", "")
            .item(SettingsAction::AddRepo, "Register another repo", "")
            .item(SettingsAction::ManageWorkspaces, "Manage workspaces", "")
            .item(SettingsAction::Reset, "Reset everything", "destructive")
            .item(SettingsAction::Cancel, "Cancel", "")
            .interact()
            .map_err(RepographError::Io)?;

        match action {
            SettingsAction::ChangeProjectRoot => {
                change_project_root(config, config_dir)?;
            }
            SettingsAction::UpdateAgents => {
                let current_set: BTreeSet<AgentId> = config
                    .agents()
                    .map(|a| a.selected.iter().copied().collect())
                    .unwrap_or_default();
                let selected = select_agents_interactively(&current_set)?;
                let scope = resolve_scope_interactive(args, &selected)?;
                config.set_agents(Some(Agents {
                    selected: selected.clone(),
                }));
                config.save(config_dir)?;
                run_install(&selected, scope, args.force)?;
                cliclack::log::success(format!(
                    "agents updated → {}",
                    if selected.is_empty() {
                        "(none)".to_string()
                    } else {
                        selected
                            .iter()
                            .map(AgentId::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ))
                .map_err(RepographError::Io)?;
            }
            SettingsAction::AddRepo => {
                let registered = register_repos_step(config, config_dir)?;
                maybe_assign_repos_to_workspaces(config, config_dir, &registered)?;
            }
            SettingsAction::ManageWorkspaces => {
                manage_workspaces(config, config_dir)?;
            }
            SettingsAction::Reset => {
                let confirm =
                    cliclack::confirm("Reset everything? This deletes all configuration.")
                        .initial_value(false)
                        .interact()
                        .map_err(RepographError::Io)?;
                if confirm {
                    *config = Config::default();
                    config.save(config_dir)?;
                    cliclack::outro("repograph reset — all configuration cleared")
                        .map_err(RepographError::Io)?;
                    return Ok(());
                }
            }
            SettingsAction::Cancel => {
                cliclack::outro("no changes").map_err(RepographError::Io)?;
                return Ok(());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsAction {
    UpdateAgents,
    ChangeProjectRoot,
    AddRepo,
    ManageWorkspaces,
    Reset,
    Cancel,
}

/// Settings-panel action: prompt the user for a new projects root and
/// persist it. Always renders the prompt (unlike the first-run step, which
/// short-circuits on an existing value). When `REPOGRAPH_PROJECT_ROOT` is
/// active, warns the user that the env var will override whatever they pick.
fn change_project_root(config: &mut Config, config_dir: &Path) -> Result<(), RepographError> {
    if std::env::var_os(PROJECT_ROOT_ENV)
        .as_ref()
        .is_some_and(|v| !v.is_empty())
    {
        cliclack::log::warning(format!(
            "{PROJECT_ROOT_ENV} is set in the environment; it overrides whatever \
             you pick here. Unset it to make the stored value effective."
        ))
        .map_err(RepographError::Io)?;
    }
    if let Some(path) = config.settings().and_then(|s| s.projects_root.as_ref()) {
        cliclack::log::info(format!("current stored value: {}", path.display()))
            .map_err(RepographError::Io)?;
    }
    ask_and_store_projects_root(config, config_dir)
}

/// First-run gate for the repo-registration step. Asks the outer
/// "Register repos now?" confirm so the user can decline the whole phase;
/// on `yes`, delegates to [`register_repos_step`].
fn maybe_register_repos(
    config: &mut Config,
    config_dir: &Path,
) -> Result<Vec<String>, RepographError> {
    let yes = cliclack::confirm("Register repos now?")
        .initial_value(false)
        .interact()
        .map_err(RepographError::Io)?;
    if !yes {
        return Ok(Vec::new());
    }
    register_repos_step(config, config_dir)
}

/// Two-phase repo registration:
///
/// 1. If a projects root is effective and has unregistered git-repo
///    children, render a `multiselect` of them (no preselection). Each pick
///    registers via [`bulk_register_path`], which uses the directory basename
///    silently on the first attempt and prompts once for an alternative name
///    on a conflict.
/// 2. Then offer a `"Register a repo at a custom path?"` confirm loop with a
///    free-form input (autocomplete) for paths outside the projects root.
///
/// Returns the names of every repo successfully registered in this step so
/// the caller can route them to the bulk-workspace assignment.
fn register_repos_step(
    config: &mut Config,
    config_dir: &Path,
) -> Result<Vec<String>, RepographError> {
    let mut registered: Vec<String> = Vec::new();
    let mut had_multiselect = false;

    if let Some(root) = effective_projects_root(config) {
        let already: BTreeSet<PathBuf> = config.repos().values().map(|r| r.path.clone()).collect();
        let candidates: Vec<PathBuf> = scan_git_repos(&root)
            .into_iter()
            .filter(|p| !already.contains(p))
            .collect();
        if !candidates.is_empty() {
            had_multiselect = true;
            let picked = pick_multiple_from_candidates(&root, &candidates)?;
            for path in picked {
                if let Some(name) = bulk_register_path(config, config_dir, &path)? {
                    registered.push(name);
                }
            }
        }
    }

    // Custom-path loop. When the multiselect didn't render (no candidates,
    // no root, all candidates already registered) we skip the first confirm
    // so the user doesn't have to say "yes" twice for a single repo.
    let mut skip_first_confirm = !had_multiselect;
    loop {
        if !skip_first_confirm {
            let prompt = if registered.is_empty() {
                "Register a repo at a custom path?"
            } else {
                "Register another repo at a custom path?"
            };
            let yes = cliclack::confirm(prompt)
                .initial_value(false)
                .interact()
                .map_err(RepographError::Io)?;
            if !yes {
                break;
            }
        }
        skip_first_confirm = false;

        let path = free_form_path_input()?;
        if let Some(name) = interactive_register_path(config, config_dir, &path)? {
            registered.push(name);
        }
    }

    Ok(registered)
}

fn pick_multiple_from_candidates(
    root: &Path,
    candidates: &[PathBuf],
) -> Result<Vec<PathBuf>, RepographError> {
    let mut multi = cliclack::multiselect::<PathBuf>(format!("Repositories in {}", root.display()))
        .required(false);
    for repo in candidates {
        let name = repo
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        multi = multi.item(repo.clone(), name, "");
    }
    multi.interact().map_err(RepographError::Io)
}

/// Bulk-register a scanned path with its basename as the name. First attempt
/// is silent (no prompt); on a `Conflict` we prompt once for an alternative
/// name. Persistent failures log a warning and skip the path so the rest of
/// the batch can proceed. Returns the registered name on success.
fn bulk_register_path(
    config: &mut Config,
    config_dir: &Path,
    path: &Path,
) -> Result<Option<String>, RepographError> {
    let default_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if default_name.is_empty() {
        cliclack::log::warning(format!(
            "skipping {}: cannot derive a name from the path",
            path.display()
        ))
        .map_err(RepographError::Io)?;
        return Ok(None);
    }

    let first_attempt = config.add_repo(
        default_name.clone(),
        Repo {
            path: path.to_path_buf(),
            description: None,
            stack: vec![],
        },
    );
    match first_attempt {
        Ok(()) => {
            config.save(config_dir)?;
            cliclack::log::success(format!("registered '{default_name}' → {}", path.display()))
                .map_err(RepographError::Io)?;
            return Ok(Some(default_name));
        }
        Err(RepographError::Conflict { kind, name }) => {
            cliclack::log::warning(format!(
                "{} conflict on {}: '{name}' already registered",
                kind,
                path.display()
            ))
            .map_err(RepographError::Io)?;
        }
        Err(e) => {
            cliclack::log::warning(format!("skipping {}: {e}", path.display()))
                .map_err(RepographError::Io)?;
            return Ok(None);
        }
    }

    // One-shot alternative-name prompt.
    let alt_input: String = cliclack::input(format!(
        "Different name for {}? (leave empty to skip)",
        path.display()
    ))
    .default_input(&default_name)
    .interact()
    .map_err(RepographError::Io)?;
    let alt_name = alt_input.trim().to_string();
    if alt_name.is_empty() || alt_name == default_name {
        cliclack::log::warning(format!("skipped {}", path.display()))
            .map_err(RepographError::Io)?;
        return Ok(None);
    }

    match config.add_repo(
        alt_name.clone(),
        Repo {
            path: path.to_path_buf(),
            description: None,
            stack: vec![],
        },
    ) {
        Ok(()) => {
            config.save(config_dir)?;
            cliclack::log::success(format!("registered '{alt_name}' → {}", path.display()))
                .map_err(RepographError::Io)?;
            Ok(Some(alt_name))
        }
        Err(e) => {
            cliclack::log::error(format!("skipped {}: {e}", path.display()))
                .map_err(RepographError::Io)?;
            Ok(None)
        }
    }
}

/// Interactive registration for a user-typed free-form path. Always prompts
/// for a name (default = basename); loops on conflict / validation failure
/// so the user can correct it. The outer caller already validated the path
/// via [`free_form_path_input`] → `validate_git_repo`, so failures here are
/// almost always name conflicts.
fn interactive_register_path(
    config: &mut Config,
    config_dir: &Path,
    path: &Path,
) -> Result<Option<String>, RepographError> {
    let default_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut current_default = default_name;
    loop {
        let name: String = cliclack::input("Name")
            .default_input(&current_default)
            .interact()
            .map_err(RepographError::Io)?;
        let name = name.trim().to_string();
        if name.is_empty() {
            cliclack::log::warning(format!("skipped {}", path.display()))
                .map_err(RepographError::Io)?;
            return Ok(None);
        }
        let repo = Repo {
            path: path.to_path_buf(),
            description: None,
            stack: vec![],
        };
        match config.add_repo(name.clone(), repo) {
            Ok(()) => {
                config.save(config_dir)?;
                cliclack::log::success(format!("registered '{name}' → {}", path.display()))
                    .map_err(RepographError::Io)?;
                return Ok(Some(name));
            }
            Err(e) => {
                cliclack::log::error(e.to_string()).map_err(RepographError::Io)?;
                current_default = name;
                // Loop and re-prompt the name.
            }
        }
    }
}

fn free_form_path_input() -> Result<PathBuf, RepographError> {
    loop {
        let raw: String = cliclack::input("Path to repository")
            .placeholder("/home/you/code/your-repo  (Tab to autocomplete)")
            .autocomplete(path_suggestions)
            .interact()
            .map_err(RepographError::Io)?;
        let path = Path::new(raw.trim());
        match validate_git_repo(path) {
            Ok(p) => return Ok(p),
            Err(e) => {
                cliclack::log::error(e.to_string()).map_err(RepographError::Io)?;
                // Loop and re-prompt.
            }
        }
    }
}

/// After repo registration, route each newly-registered repo into zero or
/// more workspaces of the user's choosing. The flow has two phases:
///
/// 1. **Workspace prep** — an optional create-new loop runs first, gated by
///    a `Create new workspaces first?` confirm when at least one workspace
///    already exists, or entered directly when none do (the outer confirm
///    already signalled intent). This seeds the target pool with anything
///    the user wants to assign into below.
/// 2. **Per-repo assignment** — for each repo in `repo_names`, render a
///    `multiselect` over the full workspace set (existing + just-created),
///    no preselection. The user picks the workspaces *that repo* should
///    join. Empty submissions are valid: that repo stays unassigned.
///
/// Each pick triggers a `Config::add_members(ws, &[repo])` call; the whole
/// step persists with a single `Config::save` at the end so partial failure
/// can't leave a half-written membership graph on disk.
///
/// No-op when `repo_names` is empty. Skipped entirely if the outer confirm
/// is declined. Final summary log enumerates the per-repo assignments.
fn maybe_assign_repos_to_workspaces(
    config: &mut Config,
    config_dir: &Path,
    repo_names: &[String],
) -> Result<(), RepographError> {
    if repo_names.is_empty() {
        return Ok(());
    }

    let n = repo_names.len();
    let outer_prompt = if n == 1 {
        format!("Add '{}' to workspaces?", repo_names[0])
    } else {
        format!("Add these {n} repos to workspaces?")
    };
    let yes = cliclack::confirm(outer_prompt)
        .initial_value(false)
        .interact()
        .map_err(RepographError::Io)?;
    if !yes {
        return Ok(());
    }

    // Phase 1: optional create-new loop, so phase 2 has targets to pick from.
    // If the registry already has workspaces, the user opts in via a confirm;
    // otherwise we enter the loop directly because there's no other path to
    // a target.
    let create_new = if config.workspaces().is_empty() {
        true
    } else {
        cliclack::confirm("Create new workspaces first?")
            .initial_value(false)
            .interact()
            .map_err(RepographError::Io)?
    };
    if create_new {
        loop {
            let ws_name = prompt_workspace_name(config)?;
            config.create_workspace(ws_name.clone(), None)?;
            let another = cliclack::confirm("Create another workspace?")
                .initial_value(false)
                .interact()
                .map_err(RepographError::Io)?;
            if !another {
                break;
            }
        }
    }

    // Phase 2: per-repo workspace assignment. The target pool is the full
    // registry of workspaces (existing + just-created). It cannot be empty
    // here: if it started empty, phase 1 forced create-new and the prompt
    // loop blocks until a valid workspace name lands.
    let all_ws: Vec<String> = config.workspaces().keys().cloned().collect();

    let mut assignments: Vec<(String, Vec<String>)> = Vec::new();
    for repo_name in repo_names {
        let mut multi = cliclack::multiselect::<String>(format!("Workspaces for '{repo_name}'"))
            .required(false);
        for ws in &all_ws {
            multi = multi.item(ws.clone(), ws.clone(), "");
        }
        let picked: Vec<String> = multi.interact().map_err(RepographError::Io)?;
        if picked.is_empty() {
            continue;
        }
        for ws in &picked {
            config.add_members(ws, std::slice::from_ref(repo_name))?;
        }
        assignments.push((repo_name.clone(), picked));
    }

    config.save(config_dir)?;

    if assignments.is_empty() {
        cliclack::log::info("no workspace assignments made").map_err(RepographError::Io)?;
    } else if assignments.len() == 1 && assignments[0].1.len() == 1 {
        let (repo, wss) = &assignments[0];
        cliclack::log::success(format!("added '{repo}' to '{}'", wss[0]))
            .map_err(RepographError::Io)?;
    } else {
        let lines: Vec<String> = assignments
            .iter()
            .map(|(repo, wss)| format!("  {repo} → {}", wss.join(", ")))
            .collect();
        cliclack::log::success(format!("workspace assignments:\n{}", lines.join("\n")))
            .map_err(RepographError::Io)?;
    }
    Ok(())
}

fn prompt_workspace_name(config: &Config) -> Result<String, RepographError> {
    loop {
        let raw: String = cliclack::input("Workspace name (lowercase, kebab-case)")
            .interact()
            .map_err(RepographError::Io)?;
        let name = raw.trim().to_string();
        if let Err(e) = validate_workspace_name(&name) {
            cliclack::log::error(e.to_string()).map_err(RepographError::Io)?;
            continue;
        }
        if config.workspaces().contains_key(&name) {
            cliclack::log::error(format!("workspace '{name}' already exists"))
                .map_err(RepographError::Io)?;
            continue;
        }
        return Ok(name);
    }
}

fn manage_workspaces(config: &mut Config, config_dir: &Path) -> Result<(), RepographError> {
    let action = cliclack::select::<WsAction>("Workspaces")
        .item(WsAction::Create, "Create", "")
        .item(WsAction::AddMembers, "Add members", "")
        .item(WsAction::RemoveMembers, "Remove members", "")
        .item(WsAction::Delete, "Delete workspace", "")
        .item(WsAction::Back, "Back", "")
        .interact()
        .map_err(RepographError::Io)?;

    match action {
        WsAction::Create => {
            let name = prompt_workspace_name(config)?;
            config.create_workspace(name.clone(), None)?;
            config.save(config_dir)?;
            cliclack::log::success(format!("workspace '{name}' created"))
                .map_err(RepographError::Io)?;
            if !config.repos().is_empty() {
                let yes = cliclack::confirm(format!("Add repos to '{name}' now?"))
                    .initial_value(true)
                    .interact()
                    .map_err(RepographError::Io)?;
                if yes {
                    add_repos_to_workspace(config, config_dir, &name)?;
                }
            }
            Ok(())
        }
        WsAction::AddMembers => {
            let Some(ws_name) = pick_existing_workspace(config, "Add members to")? else {
                return Ok(());
            };
            add_repos_to_workspace(config, config_dir, &ws_name)
        }
        WsAction::RemoveMembers => {
            let Some(ws_name) = pick_existing_workspace(config, "Remove members from")? else {
                return Ok(());
            };
            let members = config
                .resolve_workspace(&ws_name)?
                .0
                .into_iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if members.is_empty() {
                cliclack::log::warning(format!("workspace '{ws_name}' has no live members"))
                    .map_err(RepographError::Io)?;
                return Ok(());
            }
            let mut sel = cliclack::select::<String>("Repo to remove");
            for m in &members {
                sel = sel.item(m.clone(), m.clone(), "");
            }
            let repo = sel.interact().map_err(RepographError::Io)?;
            config.remove_members(&ws_name, std::slice::from_ref(&repo))?;
            config.save(config_dir)?;
            cliclack::log::success(format!("removed '{repo}' from '{ws_name}'"))
                .map_err(RepographError::Io)?;
            Ok(())
        }
        WsAction::Delete => {
            let Some(ws_name) = pick_existing_workspace(config, "Delete workspace")? else {
                return Ok(());
            };
            let confirm = cliclack::confirm(format!("Delete workspace '{ws_name}'?"))
                .initial_value(false)
                .interact()
                .map_err(RepographError::Io)?;
            if confirm {
                config.remove_workspace(&ws_name)?;
                config.save(config_dir)?;
                cliclack::log::success(format!("workspace '{ws_name}' deleted"))
                    .map_err(RepographError::Io)?;
            }
            Ok(())
        }
        WsAction::Back => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsAction {
    Create,
    AddMembers,
    RemoveMembers,
    Delete,
    Back,
}

fn pick_existing_workspace(
    config: &Config,
    prompt: &str,
) -> Result<Option<String>, RepographError> {
    if config.workspaces().is_empty() {
        cliclack::log::warning("no workspaces registered yet").map_err(RepographError::Io)?;
        return Ok(None);
    }
    let mut select = cliclack::select::<String>(prompt);
    for ws in config.workspaces().keys() {
        select = select.item(ws.clone(), ws.clone(), "");
    }
    Ok(Some(select.interact().map_err(RepographError::Io)?))
}

/// Bulk-add zero or more registered repos to a single existing workspace
/// via a `multiselect`. Repos already in the workspace are filtered out so
/// the picker only shows actionable candidates. Empty submissions are a
/// no-op (no save, no log). Singular wording when exactly one repo lands.
fn add_repos_to_workspace(
    config: &mut Config,
    config_dir: &Path,
    ws_name: &str,
) -> Result<(), RepographError> {
    let current: BTreeSet<String> = config
        .resolve_workspace(ws_name)?
        .0
        .into_iter()
        .map(|(name, _)| name.clone())
        .collect();
    let candidates: Vec<String> = config
        .repos()
        .keys()
        .filter(|n| !current.contains(n.as_str()))
        .cloned()
        .collect();
    if candidates.is_empty() {
        let reason = if config.repos().is_empty() {
            "no repos registered yet"
        } else {
            "all registered repos are already members"
        };
        cliclack::log::warning(format!("{reason} — '{ws_name}' unchanged"))
            .map_err(RepographError::Io)?;
        return Ok(());
    }
    let mut multi =
        cliclack::multiselect::<String>(format!("Repos to add to '{ws_name}'")).required(false);
    for n in &candidates {
        multi = multi.item(n.clone(), n.clone(), "");
    }
    let picked: Vec<String> = multi.interact().map_err(RepographError::Io)?;
    if picked.is_empty() {
        return Ok(());
    }
    config.add_members(ws_name, &picked)?;
    config.save(config_dir)?;
    let n = picked.len();
    let msg = if n == 1 {
        format!("added '{}' to '{ws_name}'", picked[0])
    } else {
        format!("added {n} repos to '{ws_name}'")
    };
    cliclack::log::success(msg).map_err(RepographError::Io)?;
    Ok(())
}

fn finish_outro(config: &Config, selected: &[AgentId]) -> Result<(), RepographError> {
    let agents_line = if selected.is_empty() {
        "(none)".to_string()
    } else {
        selected
            .iter()
            .map(AgentId::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let summary = format!(
        "agents:     {agents_line}\nrepos:      {}\nworkspaces: {}\n\nNext:\n  repograph status\n  repograph context  (coming soon)",
        config.repos().len(),
        config.workspaces().len(),
    );
    cliclack::outro_note("Setup complete", summary).map_err(RepographError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_empty_list_returns_empty_vec() {
        assert!(parse_agent_list("").unwrap().is_empty());
        assert!(parse_agent_list("   ").unwrap().is_empty());
    }

    #[test]
    fn parse_single_id() {
        let v = parse_agent_list("claude-code").unwrap();
        assert_eq!(v, vec![AgentId::ClaudeCode]);
    }

    #[test]
    fn parse_multiple_preserves_order() {
        let v = parse_agent_list("cursor,claude-code,agents-md").unwrap();
        assert_eq!(
            v,
            vec![AgentId::Cursor, AgentId::ClaudeCode, AgentId::AgentsMd]
        );
    }

    #[test]
    fn parse_trims_whitespace() {
        let v = parse_agent_list("  claude-code , cursor  ").unwrap();
        assert_eq!(v, vec![AgentId::ClaudeCode, AgentId::Cursor]);
    }

    #[test]
    fn parse_dedupes_while_preserving_first_occurrence() {
        let v = parse_agent_list("cursor,cursor,claude-code").unwrap();
        assert_eq!(v, vec![AgentId::Cursor, AgentId::ClaudeCode]);
    }

    #[test]
    fn parse_unknown_id_errors() {
        let err = parse_agent_list("claude-code,bogus").unwrap_err();
        match err {
            RepographError::InvalidName { kind, name, .. } => {
                assert_eq!(kind, "agent");
                assert_eq!(name, "bogus");
            }
            other => panic!("expected InvalidName, got {other:?}"),
        }
        assert_eq!(
            RepographError::InvalidName {
                kind: "agent",
                name: "bogus".into(),
                reason: "x"
            }
            .exit_code(),
            2
        );
    }

    #[test]
    fn requires_scope_predicate_matches_matrix() {
        assert!(requires_scope(&[AgentId::ClaudeCode]));
        assert!(requires_scope(&[AgentId::Windsurf]));
        assert!(requires_scope(&[AgentId::ClaudeCode, AgentId::AgentsMd]));
        assert!(!requires_scope(&[AgentId::AgentsMd]));
        assert!(!requires_scope(&[AgentId::Cursor]));
        assert!(!requires_scope(&[AgentId::Aider]));
        assert!(!requires_scope(&[AgentId::Copilot]));
        assert!(!requires_scope(&[]));
    }

    #[test]
    fn scope_bearing_names_lists_only_meaningful_agents() {
        let names = scope_bearing_names(&[
            AgentId::AgentsMd,
            AgentId::ClaudeCode,
            AgentId::Cursor,
            AgentId::Windsurf,
        ]);
        assert!(names.contains("claude-code"));
        assert!(names.contains("windsurf"));
        assert!(!names.contains("agents-md"));
        assert!(!names.contains("cursor"));
    }

    #[test]
    fn parse_scope_accepts_user_and_project() {
        assert_eq!(parse_scope("user").unwrap(), agent_artifact::Scope::User);
        assert_eq!(
            parse_scope("project").unwrap(),
            agent_artifact::Scope::Project
        );
        assert!(parse_scope("bogus").is_err());
        assert!(parse_scope("USER").is_err()); // case-sensitive
        assert!(parse_scope("").is_err());
    }
}
