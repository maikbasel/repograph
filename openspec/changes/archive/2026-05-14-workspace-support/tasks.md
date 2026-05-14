## 1. Outside-in acceptance test scaffolding

- [x] 1.1 Add `crates/repograph/tests/workspace.rs` skeleton: shared `tempdir`-based helpers (init git repo, run `repograph` with `REPOGRAPH_CONFIG_DIR`, assert exit code + stderr regex + stdout JSON).
- [x] 1.2 Add a failing acceptance test for `workspace create <name>` happy path — asserts `[workspace.<name>]` lands in `config.toml`, exit `0`, stderr confirms.
- [x] 1.3 Add failing acceptance tests for each remaining workspace subcommand (`rm`, `ls`, `show`, `add`, `remove`) covering one happy path each, scoped to red until the implementation lands.
- [x] 1.4 Add a failing acceptance test for `list --workspace <name>` happy path.
- [x] 1.5 Run `cargo test` and verify only the new tests fail (no compile errors anywhere else).

## 2. Domain layer — `Workspace` model in `repograph-core`

- [x] 2.1 Add `Workspace { description: Option<String>, members: Vec<String> }` to `crates/repograph-core/src/config.rs` with `#[serde]` annotations mirroring `Repo` (`skip_serializing_if` on `description` and empty `members`).
- [x] 2.2 Add `workspaces: BTreeMap<String, Workspace>` field to `Config` with `#[serde(rename = "workspace")]`, default-empty, skipped on serialize when empty. Add a read-only accessor `workspaces(&self)` mirroring `repos(&self)`.
- [x] 2.3 Add `validate_workspace_name(name: &str) -> Result<(), RepographError>` enforcing `^[a-z0-9][a-z0-9-]{0,62}$`, max 63 chars, reserved words `default`/`all`/`none`. Map violations to a `RepographError` variant with exit code `2`.
- [x] 2.4 Extend `RepographError` with a new `InvalidName { kind, name, reason }` variant that maps to exit code `2`. Existing `NotFound`/`Conflict` variants serve workspace cases via new `kind` discriminants ("workspace").
- [x] 2.5 Implement `Config::create_workspace(name, description) -> Result<()>` — validates name, returns `Conflict { kind: "workspace", name }` on duplicate, inserts an empty `Workspace`.
- [x] 2.6 Implement `Config::remove_workspace(name) -> Result<Workspace>` — returns `NotFound { kind: "workspace", name }` when missing, removes from map.
- [x] 2.7 Implement `Config::add_members(workspace, repos: &[String]) -> Result<()>` — atomic: validate workspace exists (NotFound if not), validate every repo name exists in `self.repos` (NotFound on first miss, no partial write), insert names into the workspace's `members`, sort + deduplicate.
- [x] 2.8 Implement `Config::remove_members(workspace, repos: &[String]) -> Result<()>` — NotFound when workspace is missing; member-not-in-list is a no-op (no error).
- [x] 2.9 Implement `Config::resolve_workspace(name) -> Result<WorkspaceResolution>` returning (live members, dangling member names) by walking the workspace's `members` and looking up each in `self.repos`. Used by `show` and `list --workspace`. (Type alias added to satisfy clippy `type_complexity`.)
- [x] 2.10 Unit tests in `crates/repograph-core/src/config.rs` covering: name validation positive/negative, reserved names, create/remove workspace, atomic add (mixed valid/invalid), idempotent add/remove, dangling resolution, round-trip stability with mixed repo+workspace entries.
- [x] 2.11 `cargo test -p repograph-core` green; `cargo clippy -p repograph-core -- -D warnings` clean.

## 3. CLI surface — `workspace` subcommand and `list --workspace` flag

- [x] 3.1 Create `crates/repograph/src/commands/workspace.rs` with a `Workspace` clap subcommand enum: `Create { name, description }`, `Rm { name }`, `Ls { json }`, `Show { name, json }`, `Add { workspace, repos: Vec<String> }`, `Remove { workspace, repos: Vec<String> }`.
- [x] 3.2 Add the new `Workspace` variant to the top-level `Command` enum in `crates/repograph/src/main.rs` and route to a `run(args, ctx) -> Result<(), RepographError>` function.
- [x] 3.3 Implement `workspace create` handler: `validate_workspace_name`, load config, call `create_workspace`, save config, `info!` log + stderr confirmation. Map errors to the documented exit codes (2 invalid, 5 conflict).
- [x] 3.4 Implement `workspace rm` handler: load config, call `remove_workspace`, save, log + stderr confirmation. NotFound → exit 3.
- [x] 3.5 Implement `workspace ls` handler: load config, decide `OutputMode` (TTY/JSON), render via new helpers in `output.rs`. Empty case yields `{ "workspaces": [] }` JSON or header-only table.
- [x] 3.6 Implement `workspace show` handler: load config, call `resolve_workspace`, build JSON envelope with `{ name, description, members: [resolved], dangling: [names] }`, render. Emit stderr `warn!` per dangling member. NotFound workspace → exit 3.
- [x] 3.7 Implement `workspace add` handler: load config, call `add_members` (atomic), save, `info!` + stderr confirmation. NotFound (workspace or any repo) → exit 3.
- [x] 3.8 Implement `workspace remove` handler: load config, call `remove_members`, save, log + stderr confirmation. NotFound (workspace only) → exit 3.
- [x] 3.9 Add `--workspace <name>` optional argument to the existing `list` command in `crates/repograph/src/commands/list.rs`. When set, call `resolve_workspace` and render only the live members; NotFound workspace → exit 3; dangling members silently skipped (no stderr warning from `list`).
- [x] 3.10 Apply `#[tracing::instrument(skip(args), fields(...))]` to every workspace `run` handler per `.claude/rules/logging.md`. Entry `debug!`, success `info!`, error `error!`.

## 4. Output rendering

- [x] 4.1 Extend `crates/repograph/src/output.rs` with `WorkspaceListEntry`/`WorkspaceListEnvelope`/`WorkspaceShowEnvelope` views carrying the data the renderers need. Serializable with `serde` so the JSON path is a direct `serde_json::to_writer`.
- [x] 4.2 Implement `render_workspaces(mode, workspaces)` — TTY table (name, description, member count) via `comfy-table` with the existing `UTF8_FULL` preset; JSON via `serde_json::to_writer` against the envelope `{ "workspaces": [...] }`. Stable alphabetical order by name (BTreeMap iteration order).
- [x] 4.3 Implement `render_workspace_show(mode, name, description, live, dangling)` — TTY table of resolved members (columns: name, path, description, stack); JSON envelope `{ name, description, members: [...], dangling: [...] }`. `dangling` field is ALWAYS present (empty array when none).
- [x] 4.4 Implement `render_repo_slice` so `list --workspace` shares the existing renderer code path via a common `render_repo_entries`. Empty filtered result yields `{ "repos": [] }` JSON or header-only table.
- [x] 4.5 Unit tests for the JSON envelope shapes (empty, populated, dangling) — assert keys are stable and never `null` where the spec requires `[]`.

## 5. Wire-up, validation, error handling

- [x] 5.1 Verify clap derive picks up `workspace` and `list --workspace` correctly via `workspace_subcommand_help_lists_all_verbs` and end-to-end usage tests. Default behavior preserved when `--workspace` is omitted.
- [x] 5.2 Verify exit-code mapping end-to-end via the acceptance tests written in step 1: 0 success, 2 invalid name / bad CLI args, 3 missing workspace / missing repo, 5 workspace create conflict. No new exit codes introduced.
- [x] 5.3 Ensure stdout/stderr discipline holds: redirect stdout in tests, assert stdout is empty or parseable JSON; redirect stderr, assert diagnostics land there. Dangling case explicitly tested — stdout JSON has `dangling: [...]`, stderr has the per-member warning.
- [x] 5.4 Ensure no `unwrap()` / `expect()` / `println!` / `eprintln!` outside test code (audited via `rg`; only `#[cfg(test)]` modules contain `.unwrap()`).

## 6. Acceptance test fill-out (red → green)

- [x] 6.1 Flesh out every spec scenario from `specs/workspace-support/spec.md` as a discrete acceptance test in `crates/repograph/tests/workspace.rs`. 38 tests cover the 45+ scenarios.
- [x] 6.2 Add tests for round-trip stability with mixed `[repo.*]` + `[workspace.*]` content: covered by `round_trip_with_mixed_repos_and_workspaces` (core) and `mixed_repo_and_workspace_round_trip_is_stable` (acceptance).
- [x] 6.3 Add tests for tombstone semantics across commands: `repo_remove_leaves_workspace_member_intact`, `dangling_member_re_registers_cleanly`, and `show_with_dangling_member_separates_live_and_tombstoned`.
- [x] 6.4 `registry_remove_behavior_unchanged_with_workspace_membership` asserts `repograph remove` is workspace-unaware — same stdout/stderr/exit code as registry-core.
- [x] 6.5 `cargo test` fully green (123 tests across the workspace); `cargo clippy --workspace --all-targets -- -D warnings` clean.

## 7. Documentation & manual validation

- [x] 7.1 `README.md` command surface table extended with the `workspace` subcommand tree and the `--workspace` flag on `list`. Exit-code table unchanged.
- [x] 7.2 `README.md` example output blocks added: workspace ls JSON, workspace show JSON with `dangling`, `list --workspace` filter usage.
- [x] 7.3 Manual smoke test with `REPOGRAPH_CONFIG_DIR=$(mktemp -d)`: registered two git repos, created workspace, added members, ran show + list --workspace, removed a repo, confirmed dangling warning + `dangling: ["api"]` JSON field, observed config.toml round-trip.
- [x] 7.4 `openspec validate workspace-support` reports "valid".

## 8. Archive readiness check (pre-archive only — do NOT archive in this change)

- [x] 8.1 Every checkbox in this `tasks.md` is ticked.
- [x] 8.2 `cargo check --workspace` warning-free, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` green (123/123).
- [x] 8.3 `design.md` reflects what was actually built. No deviations from the locked decisions; the only added detail was the `WorkspaceResolution` type alias to satisfy `clippy::type_complexity` (mentioned in task 2.9).
- [x] 8.4 `README.md` command surface + exit-code table match the implementation.
- [x] 8.5 No `unwrap()` / `expect()` outside test code (verified via `rg` — only `#[cfg(test)]` matches); no `println!` / `eprintln!` outside `output.rs` (verified); tombstone semantics intact — registry-core's `remove` is unmodified, asserted by `registry_remove_behavior_unchanged_with_workspace_membership`.
