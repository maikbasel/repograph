## 1. `edit` command — non-lossy in-place update

- [x] 1.1 Write a failing acceptance test (`assert_cmd` + `tempdir` + `git2`) for `repograph edit <name>`: description/stack change in place, nonexistent name → exit 3, no-change-flags → exit 2, non-git `--path` → exit 3
- [x] 1.2 Write a failing core test for `Config::edit_repo` covering rename (`--name`) that rewrites every `workspace.members` entry from old→new name with no dangling reference, and rename-to-existing → `Conflict`
- [x] 1.3 Implement `Config::edit_repo(&mut self, name, EditFields) -> Result<Repo, RepographError>` in `config.rs`: in-place field update, `--path` canonicalization + `validate_git_repo`, rename with workspace-member rewrite, `Conflict` on name/path collisions, `NotFound` on missing name
- [x] 1.4 Add `crates/repograph/src/commands/edit.rs` (`Args` + `run(args, config_dir)`), clap dispatch in `main.rs`, `tracing` entry/success/error, reject empty flag set with usage error (exit 2)
- [x] 1.5 Run `cargo test` for the new edit tests until green; `cargo clippy -- -D warnings` clean

## 2. `--json` confirmation envelopes on mutating commands

- [x] 2.1 Write failing acceptance tests asserting `add/remove/edit --json` and `workspace create/add/remove/rm --json` each emit a single parseable JSON object on stdout (with `action` + affected fields) and keep stdout empty without `--json`
- [x] 2.2 Add a serializable confirmation type (e.g. `MutationConfirmation` with an `action` discriminator) and a renderer in `output.rs`; emit only after the change is persisted
- [x] 2.3 Wire `--json` into `add.rs`, `remove.rs`, `edit.rs`, and the `workspace.rs` mutating subcommands; diagnostics stay on stderr
- [x] 2.4 Run the JSON-confirmation tests until green; verify each through `jq` in a test or manual check

## 3. `Capability` dimension in the artifact layer

- [x] 3.1 Write failing tests for the new emission contract in `tests/init_artifacts.rs`: `claude-code` yields a discrete `Consumer` + `Setup` artifact; `cursor` yields two `.mdc` files; flat-file agents yield one combined block; result ordering is selection-order then Consumer-before-Setup
- [x] 3.2 Add `enum Capability { Consumer, Setup }` and thread it through `resolve_path(agent, capability, scope, …)` with the setup paths (`skills/repograph-setup/SKILL.md`, `.cursor/rules/repograph-setup.mdc`)
- [x] 3.3 Extend `render_artifact(agent, capability)` — discrete body for wholly-owned-file agents; concatenated consumer+setup body for flat-file agents
- [x] 3.4 Add the `Capability` tag to `ArtifactResult` variants and update `install_artifacts` to emit one-or-two artifacts per agent per the `wholly_owned_file` predicate, in the specified order, never aborting on per-artifact failure
- [x] 3.5 Update `init.rs` call sites and any `lib.rs` re-exports; run `tests/init_artifacts.rs` until green

## 4. Setup + consumer skill bodies

- [x] 4.1 Write failing body-content tests: setup body covers `add/remove/edit/workspace` with a plan→confirm→execute→verify instruction; consumer body excludes mutating commands and delegates to `repograph-setup`; setup `SUMMARY` ≠ consumer `SUMMARY` and names register/group/update triggers
- [x] 4.2 Author `SETUP_BODY` + `SETUP_SUMMARY` consts in `agent_artifact.rs` (confirm-before-write workflow, mutating command table, `--json` verify step)
- [x] 4.3 Update the consumer `BODY`: replace the "ask the user to run `add` themselves" line with delegation to the `repograph-setup` skill; keep the read-only Commands section intact
- [x] 4.4 Keep the existing "every command name resolves to a real subcommand" test passing for both bodies; run body tests until green

## 5. Version-stamped delimiter marker

- [x] 5.1 Write failing tests: fresh install emits `<!-- repograph:begin v<N> -->`; an older-version block is rewritten in place (bytes outside delimiters preserved); identical version+body is `Unchanged`
- [x] 5.2 Introduce a body-version const and stamp it into `DELIMITER_BEGIN` rendering; make the splice detection match any `repograph:begin`/`end` pair regardless of version
- [x] 5.3 Expose a parser that extracts the installed block's version stamp (for doctor consumption); run delimiter tests until green

## 6. Doctor freshness check (read-only)

- [x] 6.1 Write failing doctor tests: missing setup artifact → `SkillArtifactFresh` `warn` with a `repograph init` hint; stale (older stamp) → `warn` naming both versions; clean current install → `ok`; no `[agents]` → check skipped; config and artifact mtimes unchanged after run
- [x] 6.2 Add the `SkillArtifactFresh` variant to the `Check` catalog enum; implement the read-only resolve-path + read-stamp + compare-to-binary-version logic (no writes), running sequentially on the main thread
- [x] 6.3 Ensure JSON envelope sorting, summary totals, and TTY table render the new findings unchanged; run doctor tests until green

## 7. Documentation and final verification

- [x] 7.1 Update `README.md`: add `edit` to the command surface, document `--json` on mutators, note the two generated skills (`repograph`, `repograph-setup`) and the doctor freshness check; exit-code table stays accurate
- [x] 7.2 Full sweep: `cargo check` (zero warnings), `cargo clippy -- -D warnings`, `cargo test` all green; no `unwrap`/`expect`/`todo!` outside tests
- [x] 7.3 Update `design.md` with any resolved deviations from the plan; run `openspec validate registry-management-skill`
