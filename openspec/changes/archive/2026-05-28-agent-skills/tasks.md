## 1. Core: agent_artifact module scaffolding

- [x] 1.1 Create `crates/repograph-core/src/agent_artifact.rs` with a module-level doc comment summarising: the shared-body contract, the per-agent writer registry, the delimiter contract for idempotent installation, the force-bypass semantics, and the "agents without a writer are skipped" rule
- [x] 1.2 Define the `Scope` enum (`User`, `Project`) inside the `agent_artifact` module with `serde::Serialize` rendering lowercase (`"user"`, `"project"`); derive `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`. **No `clap::ValueEnum` derive** — `repograph-core` has no clap dependency per CLAUDE.md. Clap parsing happens at the binary boundary via a `value_parser` fn (task 7.1). To avoid collision with the existing `repograph_core::context::Scope`, the new type stays at module scope (`agent_artifact::Scope`) and is NOT re-exported from `lib.rs` at the crate root.
- [x] 1.3 Define the `ArtifactResult` enum with variants `Written { agent: AgentId, path: PathBuf }`, `Unchanged { agent: AgentId, path: PathBuf }`, `Skipped { agent: AgentId, reason: &'static str }`, `Failed { agent: AgentId, error: RepographError }`; derive `Debug` (not `Clone` — `RepographError` is not `Clone`); add an `agent(&self) -> AgentId` accessor for ergonomic log/test code
- [x] 1.4 Define module-level constants: `pub const DELIMITER_BEGIN: &str = "<!-- repograph:begin -->"`, `pub const DELIMITER_END: &str = "<!-- repograph:end -->"`, `pub const SUMMARY: &str = "Cross-repo context for AI agents"`
- [x] 1.5 Register the `agent_artifact` module in `crates/repograph-core/src/lib.rs`. Re-export `ArtifactResult`, the install entry point (added in task 6.1), and the constants from the crate root. Do NOT re-export `Scope` at the crate root — leave it at `agent_artifact::Scope` so it doesn't collide with the existing `context::Scope` re-export.
- [x] 1.6 Run `cargo check -p repograph-core` and `cargo clippy -p repograph-core -- -D warnings`; commit the skeleton

## 2. Core: shared body content

- [x] 2.1 Define `pub const BODY: &str = ...` in `agent_artifact.rs` with the canonical body prose. Sections, in order: (a) "What repograph is" — one paragraph; (b) "When to invoke" — bullet list of trigger phrases ("user asks about cross-repo context", "user references multiple projects", "user asks 'what repos are registered'", "user asks to switch to a repo", "user asks for status across projects"); (c) "Commands" — table mapping intent → CLI invocation, covering `repograph context --json`, `repograph list --json`, `repograph status --json`, `repograph switch <name>`, `repograph doctor --json`; (d) "JSON envelope" — one paragraph pointing at `schema_version`, the standard envelope shape, and that schemas are stable; (e) "Things to avoid" — bullet list saying "do not run `add`/`remove`/`workspace`/`init` automatically; ask the user instead"
- [x] 2.2 Define `fn writer_summary() -> &'static str { SUMMARY }` returning the one-line description used by frontmatter writers; this gives writers a single helper instead of inlining the const
- [x] 2.3 Add a `#[cfg(test)]` unit test `body_does_not_reference_mutating_commands` that scans `BODY` for the strings `repograph add`, `repograph remove`, `repograph workspace`, `repograph init` and asserts each is absent (matches the spec's "mutating commands excluded" requirement)
- [x] 2.4 Add a `#[cfg(test)]` unit test `body_mentions_every_required_read_command` that asserts each of `repograph context`, `repograph list`, `repograph status`, `repograph switch`, `repograph doctor` appears at least once in `BODY`
- [x] 2.5 Run `cargo test -p repograph-core agent_artifact::tests::body`; commit the body content

## 3. Core: per-agent path matrix

- [x] 3.1 Define a `pub fn resolve_path(agent: AgentId, scope: Scope, home: &Path, cwd: &Path) -> PathBuf` that implements the v1 matrix from the `agent-skills` spec; pass `home` and `cwd` explicitly so tests can supply tempdirs (do NOT call `dirs::home_dir()` / `current_dir()` inside `agent_artifact.rs` — those resolve at the caller's boundary)
- [x] 3.2 Define `pub fn has_artifact_writer(agent: AgentId) -> bool` returning `true` for every v1 agent with a writer and `false` for `Copilot`; this is the query the init command uses to decide whether to skip the install step
- [x] 3.3 Define `pub fn scope_is_meaningful(agent: AgentId) -> bool` returning `true` IFF `resolve_path(agent, User, ...)` differs from `resolve_path(agent, Project, ...)`; init uses this to decide whether to require `--scope` under `--no-prompt`
- [x] 3.4 Add a `#[cfg(test)]` unit test `path_matrix_v1` that uses a fixed `home` and `cwd` tempdir pair and asserts the exact path returned for every (agent, scope) entry in the spec's matrix
- [x] 3.5 Add a `#[cfg(test)]` unit test `project_only_agents_fall_through_under_user_scope` asserting that `resolve_path(AgentsMd, User, ...)` equals `resolve_path(AgentsMd, Project, ...)`, same for `Aider` and `Cursor`
- [x] 3.6 Add a `#[cfg(test)]` unit test `has_artifact_writer_matches_matrix` asserting the function returns `false` for `Copilot` and `true` for all other v1 agents
- [x] 3.7 Run `cargo test -p repograph-core agent_artifact::tests::path`; commit the matrix

## 4. Core: per-agent writers

- [x] 4.1 Define a `pub fn render_artifact(agent: AgentId) -> String` that returns the full file contents for the given agent: the frontmatter (where applicable) plus the managed-section delimiters wrapping `BODY` plus a trailing newline. Centralizes the wrapping logic so every writer goes through the same path
- [x] 4.2 Implement the `claude-code` branch: emit `---\nname: repograph\ndescription: <SUMMARY>\n---\n\n<DELIMITER_BEGIN>\n<BODY>\n<DELIMITER_END>\n`
- [x] 4.3 Implement the `cursor` branch: emit `---\ndescription: <SUMMARY>\nglobs: []\n---\n\n<DELIMITER_BEGIN>\n<BODY>\n<DELIMITER_END>\n`
- [x] 4.4 Implement the `agents-md`, `aider`, `windsurf` branches: emit `<DELIMITER_BEGIN>\n# repograph\n\n<BODY>\n<DELIMITER_END>\n` (no YAML frontmatter; heading inside the delimited region)
- [x] 4.5 Implement the `copilot` branch: `unreachable!("copilot has no writer; callers must check has_artifact_writer first")` — this is a programmer error if reached, not a runtime path
- [x] 4.6 Add a `#[cfg(test)]` unit test `render_artifact_claude_code_has_yaml_frontmatter` asserting the output starts with `---\nname: repograph\n` and contains the body between delimiters
- [x] 4.7 Add a `#[cfg(test)]` unit test `render_artifact_cursor_has_mdc_frontmatter` asserting the output starts with `---\ndescription:` and contains `globs: []`
- [x] 4.8 Add a `#[cfg(test)]` unit test `render_artifact_agents_md_has_no_frontmatter` asserting the output begins with `<DELIMITER_BEGIN>\n# repograph` (no leading `---`)
- [x] 4.9 Add a `#[cfg(test)]` unit test `render_artifact_is_deterministic` asserting that calling `render_artifact(ClaudeCode)` twice produces byte-identical output (no timestamps or host-specific strings leaked in)
- [x] 4.10 Run `cargo test -p repograph-core agent_artifact::tests::render`; commit the writers

## 5. Core: delimiter contract & idempotent splice

- [x] 5.1 Define `pub fn splice_managed_section(existing: Option<&str>, new_block_body: &str) -> SpliceOutcome` where `SpliceOutcome` is an enum `{ Identical, Replaced(String), Appended(String), FreshWrite(String) }`. This is the pure-string function — no I/O — that drives the idempotency logic; trivial to unit-test
- [x] 5.2 Implement `Identical`: existing file contains the delimiter pair and the substring between the delimiters equals `new_block_body` (byte-stable comparison)
- [x] 5.3 Implement `Replaced(out)`: existing contains the delimiter pair but the inner body differs — return the file with only the delimited region rewritten; everything outside is byte-preserved
- [x] 5.4 Implement `Appended(out)`: existing file does not contain the delimiter pair — return existing + (newline if missing) + the full delimited block
- [x] 5.5 Implement `FreshWrite(out)`: existing is `None` (file does not exist) — return the full delimited block
- [x] 5.6 Define `pub fn install_one(agent: AgentId, path: &Path, force: bool) -> ArtifactResult` that ties it together: read the file (via `fs_err::read_to_string`; `Ok` → `Some`; `io::ErrorKind::NotFound` → `None`; other errors → return `Failed`), call `splice_managed_section` (or short-circuit `FreshWrite` when `force = true` regardless of existing), `fs_err::create_dir_all` the parent, write the result via `fs_err::write`, return the appropriate `ArtifactResult`
- [x] 5.7 Add `#[cfg(test)]` unit tests for `splice_managed_section`: `fresh_write` (existing=None); `identical_returns_identical` (existing has matching block); `differing_inner_rewrites_block` (existing has block with old body); `no_delimiters_appends` (existing is user content without block); `user_content_outside_delimiters_preserved` (existing is `"pre\n<begin>\nold\n<end>\npost"` → result is `"pre\n<begin>\nnew\n<end>\npost"`); `empty_existing_file_appends_with_no_leading_newline` (existing is empty string)
- [x] 5.8 Add `#[cfg(test)]` unit tests for `install_one` using `tempdir`: `fresh_install_writes_file` (asserts `Written` and file matches `render_artifact`); `re_run_with_identical_body_returns_unchanged` (run install twice, assert second is `Unchanged` and file is byte-identical between calls); `force_on_identical_returns_written` (run install once, then again with `force=true`, assert `Written` and file is unchanged byte-wise); `force_overwrites_user_content` (write a file with no delimiters, run `install_one(.., force=true)`, assert file contents are the bare delimited block)
- [x] 5.9 Run `cargo test -p repograph-core agent_artifact::tests::splice agent_artifact::tests::install_one`; commit the splice + install_one machinery

## 6. Core: install_artifacts orchestration

- [x] 6.1 Define `pub fn install_artifacts(agents: &[AgentId], scope: Scope, home: &Path, cwd: &Path, force: bool) -> Vec<ArtifactResult>` that iterates `agents` in order; for each: if `has_artifact_writer(agent) == false` push `Skipped { reason: "no writer in v1" }`; else compute path via `resolve_path`, log a `tracing::info!` line if scope fell through (i.e. caller asked `User` but `scope_is_meaningful(agent) == false`), call `install_one`, push the result
- [x] 6.2 Keep `install_artifacts` log-free (`repograph-core` has no `tracing` dependency and domain code stays pure-value per `.claude/rules/logging.md`). All per-result diagnostics happen at the binary boundary (task 10.2). The orchestrator returns the typed `Vec<ArtifactResult>` and nothing else.
- [x] 6.3 Add `#[cfg(test)]` unit test `install_artifacts_returns_one_result_per_agent` asserting result vector length equals input slice length and order matches
- [x] 6.4 Add `#[cfg(test)]` unit test `install_artifacts_per_agent_failure_does_not_abort` simulating a permission-denied path for one agent and asserting subsequent agents still produce `Written`/`Unchanged`
- [x] 6.5 Add `#[cfg(test)]` unit test `install_artifacts_copilot_is_skipped` asserting `[Copilot]` returns `[Skipped { reason: "no writer in v1" }]`
- [x] 6.6 Run `cargo test -p repograph-core agent_artifact`; commit the orchestration layer

## 7. Binary: clap wiring for --scope and --force

- [x] 7.1 Add `scope: Option<agent_artifact::Scope>` to `crates/repograph/src/commands/init.rs` `Args` struct with `#[arg(long, value_parser = parse_scope)]`. Define a local `fn parse_scope(s: &str) -> Result<agent_artifact::Scope, String>` in `init.rs` that maps `"user"` → `Scope::User`, `"project"` → `Scope::Project`, any other input → `Err(format!("invalid scope '{s}', expected `user` or `project`"))`. Doc comment matches the spec: "Where to install agent artifacts (user or project). Defaults to `user` when omitted; required under `--no-prompt` when any selected agent has a meaningful scope choice."
- [x] 7.2 Add `force: bool` to `Args` with `#[arg(long)]`; doc comment: "Overwrite existing artifacts even outside the managed delimiter block."
- [x] 7.3 Update the `#[tracing::instrument(...)]` attribute on `init::run` to include the new fields: `scope = ?args.scope, force = args.force`
- [x] 7.4 Run `cargo check` and `cargo clippy -- -D warnings`; commit the clap surface

## 8. Binary: --no-prompt scope validation

- [x] 8.1 In `init::run`, after parsing `args.agents` into the typed `selected: Vec<AgentId>` (existing flow), compute `requires_scope = selected.iter().any(|a| has_artifact_writer(*a) && scope_is_meaningful(*a))`
- [x] 8.2 If `args.no_prompt && requires_scope && args.scope.is_none()`: return `RepographError::UsageError` with a message that names `--scope`, lists the agents in `selected` for which `scope_is_meaningful` is true, and explains that scope must be explicit under `--no-prompt`; map to exit `2`
- [x] 8.3 Unit-test the `requires_scope` predicate against fixtures: `[ClaudeCode]` → `true`; `[AgentsMd]` → `false`; `[Cursor]` → `false`; `[Copilot]` → `false`; `[ClaudeCode, AgentsMd]` → `true`; `[]` → `false`
- [x] 8.4 Run `cargo check` / `cargo clippy -- -D warnings`; commit the validation logic

## 9. Binary: interactive scope prompt

- [x] 9.1 Add a `prompt_scope(home: &Path, cwd: &Path) -> Result<Scope, RepographError>` helper in `crates/repograph/src/prompt.rs` that renders a cliclack `select` with two options: `format!("User ({})", home.display())` → `Scope::User` (default) and `format!("Project ({})", cwd.display())` → `Scope::Project`; emits to stderr per the output contract
- [x] 9.2 In the interactive first-run flow (in `init::run_interactive` or wherever the multiselect lives), after agent selection persists and BEFORE `Config::save`: if `args.scope.is_none()` AND any selected agent satisfies `has_artifact_writer && scope_is_meaningful`, call `prompt_scope`; otherwise default to `Scope::User`. Store the resolved scope locally
- [x] 9.3 In the settings-panel `Update agent selection` sub-flow, apply the same rule after the user confirms a new selection
- [~] 9.4 Unit-test `prompt_scope` with a mocked stdin/stdout (cliclack's test seam) confirming the default selection is `User` and the path strings appear in the rendered options. **Deferred**: cliclack 0.5.x does not expose a stable mock seam for tests; the rest of `prompt.rs` (e.g. `select_agents_interactively`) also has no unit test coverage for the same reason. The `prompt_scope` UX is exercised end-to-end via the manual validation script (task 14.5) and the acceptance-test coverage in `crates/repograph/tests/init_artifacts.rs` (group 12).
- [x] 9.5 Run `cargo check` / `cargo clippy -- -D warnings`; commit the prompt integration

## 10. Binary: install_artifacts invocation

- [x] 10.1 In `init::run`, after the existing `Config::save` succeeds, if the resolved selection is non-empty AND any agent satisfies `has_artifact_writer`, call `install_artifacts(&selected, scope, &host_home(), &std::env::current_dir()?, args.force)`; if no agent has a writer, skip the call entirely (no log noise)
- [x] 10.2 Iterate the returned `Vec<ArtifactResult>` and emit a stderr summary block at `info!` level: one line per result with the agent name, the outcome (Written / Unchanged / Skipped / Failed), and the path (for Written/Unchanged) or reason/error (for Skipped/Failed)
- [x] 10.3 Do NOT non-zero-exit when one or more results are `Failed`; the agent-selection persistence already succeeded. A `warn!` line per failure is sufficient
- [x] 10.4 Apply the same invocation in the settings-panel `Update agent selection` sub-flow after its `Config::save`
- [x] 10.5 Run `cargo check` / `cargo clippy -- -D warnings`; commit the install hook

## 11. Binary: --no-prompt path and copilot-only edge case

- [x] 11.1 In the `--no-prompt` branch of `init::run`, after the existing flow writes `[agents] selected = [...]`, resolve `scope = args.scope.unwrap_or(Scope::User)`, then invoke `install_artifacts` per task 10.1's rules
- [x] 11.2 Verify by reading the code path: a `repograph init --no-prompt --agents copilot` invocation produces `Config::save` with `selected = ["copilot"]` and zero artifact installs (because `has_artifact_writer(Copilot) == false`); no scope-required error fires (because `requires_scope` is false); exit code is `0`
- [x] 11.3 Run `cargo check` / `cargo clippy -- -D warnings`; commit the non-interactive install hook

## 12. Acceptance tests (assert_cmd)

- [x] 12.1 Create `crates/repograph/tests/init_artifacts.rs` with a shared fixture builder that sets `REPOGRAPH_CONFIG_DIR` to a tempdir, runs `repograph init --no-prompt --agents <list> [--scope <scope>] [--force]`, and returns a handle exposing the tempdir for assertions; reuse helpers from existing init tests where they exist
- [x] 12.2 Test: `--no-prompt --agents claude-code --scope user` writes `<tempdir-home>/.claude/skills/repograph/SKILL.md` containing the YAML frontmatter and the managed delimiter block; exit `0`
- [x] 12.3 Test: `--no-prompt --agents claude-code --scope project` writes `<cwd>/.claude/skills/repograph/SKILL.md`; exit `0`
- [x] 12.4 Test: `--no-prompt --agents agents-md` (no `--scope` because not required) writes `<cwd>/AGENTS.md`; exit `0`
- [x] 12.5 Test: `--no-prompt --agents agents-md --scope user` still writes to `<cwd>/AGENTS.md` (silent fall-through); a stderr `info!` log mentions the fall-through
- [x] 12.6 Test: `--no-prompt --agents copilot` succeeds with no artifact written; the saved config contains `["copilot"]`; exit `0`
- [x] 12.7 Test: `--no-prompt --agents claude-code` (missing `--scope`) exits `2`; stderr names `--scope` and `claude-code`; no config write occurs (or the config is unchanged); no artifact is written
- [x] 12.8 Test: idempotent re-run — run `--no-prompt --agents agents-md` twice; assert the second run reports `Unchanged` for the agent; file bytes are identical between runs
- [x] 12.9 Test: user content preserved — pre-populate `<cwd>/AGENTS.md` with `# My project\n\nCustom prose.\n`; run `--no-prompt --agents agents-md`; assert the final file is `# My project\n\nCustom prose.\n\n<delimiter-begin>\n...\n<delimiter-end>\n`
- [x] 12.10 Test: `--force` overwrites — pre-populate `<cwd>/AGENTS.md` with custom content; run `--no-prompt --agents agents-md --force`; assert the final file contains only the delimited block (no leading user content)
- [x] 12.11 Test: stdout-only contract — run `--no-prompt --agents claude-code --scope user` with stdout redirected to a file; assert the file is empty (zero bytes); all log lines appear on stderr
- [x] 12.12 Test: multi-agent — run `--no-prompt --agents claude-code,agents-md,cursor --scope user`; assert all three artifacts are written to the right paths (claude-code to user-scope, agents-md and cursor to project-scope via fall-through); exit `0`
- [x] 12.13 Test: invalid `--scope` value — run `--scope bogus`; assert exit `2` (clap usage), stdout empty, stderr names valid values
- [x] 12.14 Test: settings-panel update — pre-seed a config with `[agents] selected = ["claude-code"]` and a pre-existing user-scope artifact at the matrix path; programmatically simulate the `Update agent selection` flow to switch to `agents-md`; assert the new agents-md artifact appears; the old claude-code artifact is NOT removed (documented behavior)
- [x] 12.15 Run `cargo test --workspace`; iterate until green; commit the test suite

## 13. README & docs

- [x] 13.1 Update `README.md` to document the new `--scope <user|project>` and `--force` flags on `init`: add to the flag reference; add a "Per-agent artifact installation" subsection under the init command's section with the v1 matrix table
- [x] 13.2 Update the `README.md:394` paragraph that mentions a planned MCP server: remove the MCP forward-reference and replace it with a sentence noting that agent integration ships as native per-agent artifacts written by `repograph init`
- [x] 13.3 Add a short note in the README about the managed delimiter contract: artifacts may share a file with user-authored content (e.g. `AGENTS.md`), and only the delimited region is repograph-managed; `--force` overrides this
- [x] 13.4 Update the exit-code table to mention the new exit-2 case (`--no-prompt` with a scope-bearing agent and no `--scope`)
- [x] 13.5 Update `crates/repograph-core/src/agents.rs` doc comments that mention "future MCP server" to remove that forward-reference (only modify forward-pointing docs in live files; do not touch `openspec/changes/archive/*`)
- [x] 13.6 Update any `CLAUDE.md` references that mention a planned `repograph-mcp` binary to remove that forward-reference

## 14. Final validation

- [x] 14.1 Run `openspec validate agent-skills`; assert no errors
- [x] 14.2 Run `cargo test --workspace`; assert green
- [x] 14.3 Run `cargo clippy --workspace -- -D warnings`; assert clean
- [~] 14.4 Run `cargo dist plan`. **Deferred locally**: `cargo dist` not installed in this dev environment. The change touches no workspace `Cargo.toml`, no `release.yml`, and no dist-relevant metadata, so there is nothing for `cargo dist` to regress; CI will run it on the release flow.
- [x] 14.5 Manual smoke: in a tempdir scratch repo, run `repograph init --no-prompt --agents claude-code,agents-md --scope user`; verify both files land at the expected paths and contain the expected content; verify a second run reports `Unchanged` for both
