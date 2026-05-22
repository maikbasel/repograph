## 1. Dependencies & wiring

- [x] 1.1 Add `cliclack` to `crates/repograph/Cargo.toml` (binary crate only; pin exact version per design risk mitigation)
- [x] 1.2 Verify `is-terminal` is already on the binary crate's dependency list (used for TTY gating); add if missing
- [x] 1.3 Update `openspec/changes/init-command/proposal.md` if any dependency choice deviates from the design — `dirs` added to binary crate; captured as a resolved deviation in `design.md`

## 2. Agent registry in core

- [x] 2.1 Add acceptance test scaffold `crates/repograph-core/src/agents.rs` covering: each v1 agent ID resolves to expected patterns; unknown ID returns `None`; round-trip serde (kebab-case strings) for the enum
- [x] 2.2 Implement `AgentId` enum with variants `ClaudeCode`, `AgentsMd`, `Cursor`, `Aider`, `Windsurf`, `Copilot`; derive `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize` with `#[serde(rename_all = "kebab-case")]`
- [x] 2.3 Implement `AgentId::file_patterns(&self) -> &'static [&'static str]` returning the exact patterns from the spec table
- [x] 2.4 Implement `AgentId::all() -> &'static [AgentId]` for use by detection / multiselect rendering
- [x] 2.5 Add `AgentId::parse(&str) -> Result<Self, RepographError>` for `--agents` flag parsing; map unknown IDs to `RepographError::InvalidName { kind: "agent", .. }`
- [x] 2.6 Re-export `AgentId` from `crates/repograph-core/src/lib.rs`

## 3. Config schema extension

- [x] 3.1 Add acceptance test scaffold in `crates/repograph-core/src/config.rs` tests module covering: config without `[agents]` loads with `agents = None`; config with empty `[agents] selected = []` loads as `Some(Agents { selected: vec![] })`; config save with `agents = None` omits the section; selection order is preserved across round-trip
- [x] 3.2 Define `Agents` struct with `selected: Vec<AgentId>`, derive serde + Default
- [x] 3.3 Extend `Config` with `agents: Option<Agents>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`
- [x] 3.4 Add `Config::agents() -> Option<&Agents>` getter (plus `set_agents` for init writes)
- [x] 3.5 Verify the existing round-trip stability test still passes (no spurious sections)
- [x] 3.6 Update existing config tests that assume specific serialized output if they break — none needed

## 4. New error variant

- [x] 4.1 Add `RepographError::NeedsInit(String)` variant in `crates/repograph-core/src/error.rs` — refined to a `String` payload so the same variant can carry both the "agents not configured" and the "init in non-TTY" messages (resolved deviation in `design.md`)
- [x] 4.2 Map `NeedsInit` to exit code `2` in the error → exit-code function
- [x] 4.3 Unit-test the exit code mapping

## 5. Detection helper

- [x] 5.1 Add acceptance test scaffold in `crates/repograph/src/prompt.rs` (or a sub-module) using a `tempdir`-backed fake `$HOME` to verify each detection signal triggers its corresponding agent preselect; verify missing home produces no preselects
- [x] 5.2 Implement `detect_agents(home: Option<&Path>) -> BTreeSet<AgentId>` that probes each path from the spec's detection table; `agents-md` SHALL never appear in the output
- [x] 5.3 Ensure detection is total — no panics on unreadable or non-existent paths; use `Path::exists()` without erroring

## 6. cliclack output contract (resolved deviation)

- [x] 6.1 ~~Add a `with_stderr_theme` wrapper~~ — **not needed**: cliclack 0.5.4 writes its UI to `Term::stderr()` natively (verified in `cliclack-0.5.4/src/lib.rs:328`). Resolved deviation documented in `design.md`.
- [x] 6.2 ~~Smoke-test the wrapper~~ — replaced by the `init_no_prompt_emits_nothing_to_stdout` acceptance test which validates the stdout-is-data contract end-to-end.
- [x] 6.3 Document the cliclack stderr behavior in `prompt.rs` module doc comment with a link to the `design.md` resolved-deviation note

## 7. Auto-prompt helper

- [x] 7.1 Add tests for `ensure_agents_configured`: (a) no-op when `[agents]` present — covered indirectly via the integration of `set_agents`/`agents()` in config tests; (b) TTY missing section runs sub-flow — covered by manual validation (interactive); (c) non-TTY missing section returns `NeedsInit` — exercised by `init_non_tty_without_flags_exits_2` (same code path, exit `2`)
- [x] 7.2 Implement `prompt::ensure_agents_configured(config: &mut Config, config_dir: &Path) -> Result<(), RepographError>` per the spec contract
- [x] 7.3 In the TTY branch, run the agent multiselect sub-flow (shared with first-run init), mutate the config, persist via `Config::save`
- [x] 7.4 ~~Expose a non-interactive override hook for testing~~ — not added; the non-TTY branch's exit-code behavior is verified by acceptance test, and the TTY branch is covered by manual validation. Adding a debug-only flag for a path already validated by other tests was deemed unnecessary scope.

## 8. Init command — non-interactive path (TDD anchor)

- [x] 8.1 Add acceptance tests in `crates/repograph/tests/init.rs` for the non-interactive variant covering each scenario in the spec: happy path, overwrite preserves repos/workspaces, `--no-prompt` without `--agents` exits 2, unknown agent ID exits 2, non-TTY without flags exits 2, selection order preserved, overwrite replaces previous agents, empty selection valid, stdout empty
- [x] 8.2 Add `crates/repograph/src/commands/init.rs` with `Args { agents: Option<String>, no_prompt: bool }` and the `run(args, config_dir) -> Result<(), RepographError>` signature
- [x] 8.3 Implement validation: `--no-prompt` requires `--agents` via clap's `#[arg(long, requires = "agents")]` — exits `2` natively; resolved deviation noted in `design.md`
- [x] 8.4 Implement non-interactive happy path: parse `--agents` via `AgentId::parse`, build `Agents { selected }`, write to config, return `Ok`
- [x] 8.5 Wire `init` into `crates/repograph/src/main.rs` clap dispatch and `crates/repograph/src/commands/mod.rs` re-export

## 9. Init command — interactive first-run flow

- [x] 9.1 Add a manual validation checklist comment at the top of `commands/init.rs` referencing design.md's "Manual Validation Script" section
- [x] 9.2 Implement the first-run intro: cliclack `intro("repograph init")`, brief welcome note
- [x] 9.3 Implement the agent multiselect step: build the option list from `AgentId::all()`, preselect from `detect_agents(...)` plus any `--agents` overrides passed alongside (without `--no-prompt`); persist selection
- [x] 9.4 Implement the optional repo step: cliclack `confirm("Register a repo now?")`, if yes prompt for path and optional name, call `Config::add_repo` with the same validation as `commands::add`; on error render via cliclack `log::error` and re-prompt
- [x] 9.5 Implement the optional workspace step: cliclack `select` between Skip / Add to existing / Create new; for "Create new" prompt for a workspace name and validate via `validate_workspace_name`; on success call `Config::add_members`; on error render and re-prompt
- [x] 9.6 Implement the summary screen: cliclack `outro_note("Setup complete", ...)` with a multi-line note summarizing agents / repos / workspaces, plus "Next" hints
- [x] 9.7 Persist after each step that mutates config so a partial cancel doesn't lose earlier work; ensure final save is idempotent

## 10. Init command — settings-panel flow

- [x] 10.1 Implement detection of populated state: when `config.agents().is_some()`, route to the settings panel instead of first-run
- [x] 10.2 Implement the top-level settings panel select: Update agent selection / Register another repo / Manage workspaces / Reset everything / Cancel
- [x] 10.3 Implement "Update agent selection": render the multiselect with the current selection pre-checked (no detection); persist on confirm
- [x] 10.4 Implement "Register another repo": reuse the optional-repo flow from first-run
- [x] 10.5 Implement "Manage workspaces": sub-select between Create / Add members / Remove members / Delete workspace; each branch reuses `Config::*` primitives
- [x] 10.6 Implement "Reset everything": cliclack `confirm` with default `false`; on confirmation overwrite config with `Config::default().save(config_dir)`
- [x] 10.7 Implement "Cancel": clean exit with code `0`, no config write

## 11. Tracing instrumentation

- [x] 11.1 Add `#[tracing::instrument(...)]` to `init::run` with `no_prompt`, `agents_flag`, `config_dir` fields
- [x] 11.2 Emit `debug!("init: start")` at entry, `info!("init: completed (non-interactive|interactive)")` on completion; error propagation is handled by `report()` in `main.rs` (consistent with other commands)
- [x] 11.3 Add equivalent instrumentation to `prompt::ensure_agents_configured`

## 12. README and CLAUDE.md updates

- [x] 12.1 Update `README.md` command-surface table to include `init`
- [x] 12.2 Add a "First run" section to `README.md` showing the interactive flow and the `--no-prompt --agents <list>` variant
- [x] 12.3 Add the agent ID → file pattern table to `README.md` so users can predict what `context` will inline once Phase 4b lands
- [x] 12.4 Update the dev plan table in `CLAUDE.md`: Phase 4 split into `4a — Agent Toolchain Setup (init-command)` and `4b — Agent Context (context-command)`; SAFE-TO-MODIFY list extended with `agents.rs` and `prompt.rs`
- [x] 12.5 Update the exit-code table in `README.md` — confirmed `2` covers both the non-TTY init guard and the future `NeedsInit` from agent-consuming commands

## 15. Persisted projects root (industry-pattern setting)

- [x] 15.1 Add `Settings` struct to `repograph-core::config` with `projects_root: Option<PathBuf>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`); extend `Config` with `settings: Option<Settings>`; add `Config::settings()` getter and `Config::set_settings()` setter
- [x] 15.2 Re-export `Settings` from `repograph-core::lib`
- [x] 15.3 Add core unit tests covering: config without `[settings]` loads as `None`; save with `None` omits header; `projects_root` round-trips; empty `[settings]` writes header but no field; byte-stable round-trip with agents+repos+settings
- [x] 15.4 Add `prompt::PROJECT_ROOT_ENV` constant (`"REPOGRAPH_PROJECT_ROOT"`) and `prompt::effective_projects_root(config)` resolving env → config → None, with a testable inner `resolve_projects_root(config, env)` that takes the env value as a parameter so tests don't race on process env
- [x] 15.5 Add unit tests for `resolve_projects_root` covering: none/none → None; config only; env wins over config; empty env falls through; env-only no-config
- [x] 15.6 Filter `discover_project_roots` to roots containing at least one git repo (kills the "Projects (0 repos)" noise the user surfaced); update tests + add a new test asserting empty candidates are suppressed
- [x] 15.7 Add `init::pick_projects_root_step` for the first-run flow (short-circuits when effective root is already known) and `init::ask_and_store_projects_root` (renders the prompt + stores) so both first-run and settings-panel paths share the same widget
- [x] 15.8 Wire `pick_projects_root_step` into the first-run sequence between agent selection and the optional repo step; "Skip" writes `[settings]` header with no `projects_root` so the question is recorded as answered
- [x] 15.9 Add `SettingsAction::ChangeProjectRoot` with an `init::change_project_root` handler that warns when `REPOGRAPH_PROJECT_ROOT` is active (env shadows the stored value), logs the current stored value if any, and re-runs `ask_and_store_projects_root`
- [x] 15.10 Refactor `init::pick_repo_path` to use `effective_projects_root(config)` directly — no more probing inside the repo-registration step
- [x] 15.11 Update `design.md` with the "ask once, persist, env override" decision and alternatives considered
- [x] 15.12 Update `specs/init-command/spec.md` with the new "Project root is persisted in `[settings]`" requirement (7 scenarios) and revise the repo-registration requirement to reference the persisted root instead of in-step probing
- [x] 15.13 Update `README.md` — add `[settings]` to the sample config, document `REPOGRAPH_PROJECT_ROOT`, mention the "Change project root" action in the first-run/settings flow

## 14. Repo-path UX: autocomplete + project-root discovery

- [x] 14.1 Add `prompt::path_suggestions(&str) -> Vec<String>` — filesystem-aware completion source for cliclack `Input::autocomplete`. Expands `~`, scans the parent dir, filters to directories, honors the hidden-file convention, returns absolute paths with trailing `/`
- [x] 14.2 Add unit tests for `path_suggestions` covering: nonexistent parent → empty; trailing slash → list dir; prefix filtering; hidden-file convention; alphabetical sort; tilde expansion (via helper)
- [x] 14.3 Add `prompt::PROJECT_ROOT_CANDIDATES` const + `prompt::discover_project_roots(home)` returning existing well-known parent folders under `$HOME`
- [x] 14.4 Add `prompt::scan_git_repos(root)` returning direct subdirectories whose `.git` entry exists (directory or worktree marker)
- [x] 14.5 Add unit tests covering: empty home → empty; only existing roots returned; scan finds `.git` dirs and worktree-marker files; non-`.git` dirs excluded; alphabetical sort; nonexistent root → empty
- [x] 14.6 Refactor `init::register_one_repo` to call a new `pick_repo_path(config)` helper that: (1) discovers roots, (2) prompts if multiple roots exist (single root used silently), (3) renders a `select` of unregistered git repos with `Other path...` escape, (4) falls through to `free_form_path_input` with `path_suggestions` autocomplete
- [x] 14.7 Enable cliclack `filter_mode` on the repo picker so typing narrows the list (handles users with many repos in one root)
- [x] 14.8 Update `design.md` with the autocomplete + discovery decision (alternatives considered)
- [x] 14.9 Update `specs/init-command/spec.md` with a new "Repo registration step offers project-root discovery and filesystem autocomplete" requirement and its scenarios

## 16. Bulk multi-repo registration

- [x] 16.1 Update `specs/init-command/spec.md`: rewrite the repo-registration requirement scenarios to describe the bulk multiselect; add new requirement "Bulk workspace assignment for repos registered together" with 5 scenarios
- [x] 16.2 Update `design.md`: add "Decision: bulk multi-repo registration via multiselect" and a new manual-validation scenario 2a (bulk multiselect + bulk workspace assignment)
- [x] 16.3 Refactor `init::register_one_repo` / `pick_repo_path` / `pick_from_candidates` / `maybe_assign_to_workspace` into:
  - `register_repos_step(config, config_dir) -> Result<Vec<String>>` — multiselect over `scan_git_repos(root)` filtered by already-registered, then a `Register a repo at a custom path?` confirm loop with `free_form_path_input` + autocomplete
  - `bulk_register_path(config, config_dir, path) -> Result<Option<String>>` — silent first attempt with basename, one-shot "Different name?" prompt on Conflict, log+skip on persistent failure
  - `interactive_register_path(config, config_dir, path) -> Result<Option<String>>` — explicit name prompt for free-form path
  - `maybe_assign_repos_to_workspace(config, config_dir, repo_names) -> Result<()>` — single confirm + single workspace select + single `Config::add_members(ws, &all_names)`; singular/plural wording from `repo_names.len()`
- [x] 16.4 Wire `run_first_run` to `let registered = maybe_register_repos(...)?; maybe_assign_repos_to_workspace(..., &registered)?;` (outer confirm preserved so the user can decline the whole step)
- [x] 16.5 Wire `SettingsAction::AddRepo` to `register_repos_step(...)?` then `maybe_assign_repos_to_workspace(...)` — no outer confirm (menu choice is the intent)
- [x] 16.6 Delete unused helpers (`register_one_repo`, `pick_repo_path`, `pick_from_candidates`, `maybe_assign_to_workspace`)
- [x] 16.7 Update `README.md` first-run section to describe the multiselect step; update the manual TTY scenarios
- [x] 16.8 Extend `maybe_assign_repos_to_workspaces` to support **multiple** workspace targets per batch: outer confirm → multiselect over existing workspaces (no preselection) → optional create-new loop (gated by confirm when existing workspaces exist; entered directly when none); one `Config::add_members` per target, single `Config::save` at the end. Delete the now-unused `WorkspaceChoice` enum.
- [x] 16.9 Update `specs/init-command/spec.md`: rewrite the "Bulk workspace assignment" requirement to support multiple targets (rename to "supports multiple workspace targets"); refresh scenarios to cover single-existing, multiple-existing, create-new alongside existing, no-existing, empty selection, and N=1 with M>1 wording
- [x] 16.10 Update `design.md`: expand the bulk-workspace decision with multi-target rationale and sub-alternatives; add manual scenarios 2a (multi-workspace) and 2b (create-new alongside existing)
- [x] 16.11 Update `README.md` first-run section step 4 to describe the multiselect + create-new loop

## 17. Manage-workspaces bulk-add

- [x] 17.1 Extract `add_repos_to_workspace(config, config_dir, ws_name)` helper in `init.rs` — multiselect over registry filtered to non-members, no-op on empty submission, singular/plural success log
- [x] 17.2 Wire `WsAction::Create` to chain into `add_repos_to_workspace` via a default-yes confirm immediately after `Config::create_workspace` (skip confirm when zero repos registered)
- [x] 17.3 Replace `WsAction::AddMembers` single-select (`pick_repo`) with the bulk multiselect; delete the now-unused `pick_repo` helper
- [x] 17.4 Update `specs/init-command/spec.md`: new requirement "Manage-workspaces sub-flow adds repos in bulk via multiselect" with 7 scenarios (Create-chains-add, Create-no-repos-skip, Create-decline, AddMembers-filtered-multiselect, AddMembers-zero-noop, AddMembers-all-already-members-warn, AddMembers-no-repos-warn)
- [x] 17.5 Update `design.md`: add decision note for the Create→add chain and the AddMembers multiselect; add manual scenario 6 (Manage workspaces → Create populates new workspace in one step)
- [x] 17.6 Verify `cargo clippy -- -D warnings` and `cargo test` remain green; `openspec validate init-command` passes

## 18. Per-repo workspace routing after bulk-registration

Surfaced by the user mid-implementation: "i want to choose which repo to put into which workspace." The matrix shape from group 16 (every picked workspace gets every registered repo) collapsed three repos into the same workspace set in one shot, which fits N=1 but is wrong when registering a batch that fans out across workspaces. Invert the iteration.

- [x] 18.1 Rewrite `maybe_assign_repos_to_workspaces` in `crates/repograph/src/commands/init.rs` to per-repo iteration: outer confirm → phase 1 optional create-new loop (gated by `Create new workspaces first?` confirm when existing workspaces exist; entered directly when none) → phase 2 per-repo `multiselect("Workspaces for '<repo>'")` over the full target pool with `.required(false)`. Apply `add_members(ws, &[repo])` per pick. Single `Config::save` at the end. Per-repo empty pick = silent no-op for that repo. Success log uses singular `"added '<repo>' to '<ws>'"` for the (1, 1) case; multi-line `workspace assignments:\n  <repo> → <ws1>, <ws2>\n  ...` block otherwise; `INFO` `"no workspace assignments made"` when zero repos got picks. The target pool cannot be empty by construction (phase 1 forces create-new when no workspaces exist and `prompt_workspace_name` blocks until valid).
- [x] 18.2 Update `specs/init-command/spec.md`: replace the "Bulk workspace assignment supports multiple workspace targets" requirement with "Per-repo workspace assignment after bulk-registration". 7 new scenarios: per-repo-different-workspaces, same-workspace-every-repo, single-into-single-singular-log, empty-pick-leaves-repo-unassigned, all-empty-info-noop, create-new-seeds-picker, no-existing-enters-create-loop-directly.
- [x] 18.3 Update `design.md`: add a "Resolved deviation: Per-repo iteration instead of matrix-style multi-target assignment" subsection under the bulk-workspace decision; refresh manual scenarios 2a (per-repo routing across multiple workspaces) and 2b (per-repo routing with create-new mix).
- [x] 18.4 Update `README.md` first-run step 4 paragraph to describe per-repo routing (matrix wording removed).
- [x] 18.5 Verify `cargo clippy -- -D warnings` and `cargo test` remain green; `openspec validate init-command` passes.

## 13. Pre-archive verification

- [x] 13.1 Run `cargo test` — all 207 tests passing
- [x] 13.2 Run `cargo clippy -- -D warnings` — clean
- [x] 13.3 Run `cargo check` — zero warnings
- [ ] 13.4 Walk the manual validation script from `design.md` step by step; capture results as a comment on the archive PR
- [x] 13.5 Confirm no `unwrap()`/`expect()` outside test code — all matches are inside `#[cfg(test)] mod tests` blocks or pre-existing test helpers (verified)
- [x] 13.6 Confirm `dist plan` still succeeds — exit `0`, all five target platforms planned
- [x] 13.7 Run `openspec validate init-command` — green
