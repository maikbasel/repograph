## Why

repograph's agent-context capability needs a contract for *which* docs to inline per repo. Hardcoding "always CLAUDE.md" excludes users of Cursor, Aider, Windsurf, Copilot, or the cross-vendor AGENTS.md convention; per-repo configuration knobs spread the contract thin. The honest model is that the user declares their agent toolchain once, and repograph knows where each toolchain stores its rules. That declaration deserves a first-class onboarding flow — a polished `repograph init` that detects what's available, confirms with the user, and writes the result — rather than a hidden flag or a manual TOML edit. It also unblocks Phase 4b (`context-command`), which composes against the declared agent set without needing its own discovery logic.

## What Changes

- Add new `repograph init` subcommand orchestrating a multi-step guided flow (Ultracite / `@clack/prompts` aesthetic) using the `cliclack` crate.
- Add new `[agents]` section to the config TOML: `selected: Vec<AgentId>`. Presence of the section signals "init has been run"; missing section triggers an auto-prompt fallback. Empty `selected = []` is a valid configured state (user opted out of agent docs).
- Add new built-in agent registry in `repograph-core` mapping agent IDs → file pattern(s): `claude-code` → `CLAUDE.md`; `agents-md` → `AGENTS.md`; `cursor` → `.cursor/rules/*.md` + `.cursorrules`; `aider` → `CONVENTIONS.md`; `windsurf` → `.windsurfrules`; `copilot` → `.github/copilot-instructions.md`.
- Detection step preselects agents based on `~/.claude/`, `~/.cursor/`, `~/.aider/`, `~/.config/github-copilot/` presence. User can deselect anything — detection only suggests.
- Init flow composes existing capabilities: agent multiselect → optional first-repo registration (reuses `Config::add_repo` from `registry-core`) → optional workspace assignment (reuses `Config::create_workspace` / `Config::add_members` from `workspace-support`) → polished summary screen.
- Re-init flow: when `[agents]` already exists, `repograph init` renders a "settings panel" — update agent selection / register another repo / manage workspaces / reset everything / cancel.
- Non-interactive variant: `--agents claude-code,cursor --no-prompt` for CI / automation; mandatory when stdout is not a TTY.
- Add shared auto-prompt helper (in the binary crate) consumed by future commands: if `[agents]` missing and stdout is a TTY, runs the agent-selection sub-flow inline; if non-TTY, exits with a clean "run `repograph init` in an interactive shell" message and exit code `2`.
- Add new dependency: `cliclack` (binary crate only; core stays terminal-free).
- Update the dev plan in `CLAUDE.md` to reflect the Phase 4 split: 4a `init-command` (this change), 4b `context-command` (next).

## Capabilities

### New Capabilities

- `init-command`: Interactive setup command. Owns the `[agents]` config schema, the agent ID → file pattern registry, the multi-step cliclack flow (detection, selection, optional first-repo + workspace composition, summary), the re-init "settings panel" mode, the non-interactive `--agents`/`--no-prompt` mode, the shared auto-prompt fallback helper for non-interactive consumers, and the exit-code mapping for the new failure modes (e.g. non-TTY without flags).

### Modified Capabilities

(none — `[agents]` is additive to the registry-core config schema, but the requirements of `registry-core`, `workspace-support`, and `git-status` do not change. Init composes their existing primitives without modifying them.)

## Impact

- **Code**:
  - New: `crates/repograph-core/src/agents.rs` (agent ID enum, file patterns, registry; no I/O).
  - Modified: `crates/repograph-core/src/config.rs` (add `Agents` struct, `selected: Vec<AgentId>`, getter, serde round-trip; preserve unknown fields per the existing tolerance contract).
  - Modified: `crates/repograph-core/src/lib.rs` (re-export agent types).
  - New: `crates/repograph/src/commands/init.rs` (clap `Args` + `run()` orchestrating the flow).
  - New: `crates/repograph/src/prompt.rs` (cliclack scaffolding + TTY-gated auto-prompt helper, kept distinct from `output.rs`).
  - Modified: `crates/repograph/src/main.rs` (wire `init` subcommand).
  - Modified: `crates/repograph/src/commands/mod.rs` (re-export `init`).
- **Dependencies**: Add `cliclack` to `crates/repograph/Cargo.toml` only. Workspace `Cargo.toml` gets the version pinning per existing convention.
- **Config schema**: Additive only. Existing configs without `[agents]` are valid and trigger first-run prompt the next time an agent-consuming command runs.
- **Exit codes**: Reuses the existing contract (0 success; 2 usage error — used for non-TTY without `--agents`; 4 permission denied on config write; 5 conflict for repo/workspace conflicts surfaced during composed `add` / `workspace create`). No new exit codes.
- **Docs**: `README.md` gets the `init` command surface, the agent ID table, and a "first run" example. `CLAUDE.md` dev plan table updated.
- **Tests**: Acceptance tests in `crates/repograph/tests/` covering the non-interactive (`--agents`, `--no-prompt`) path via `assert_cmd` against a `tempdir`-backed config. Unit tests for the agent registry and `Agents` config serde round-trip. The interactive cliclack flow itself is covered by manual validation (documented in design.md).
- **Downstream**: Unblocks `context-command` (Phase 4b). The auto-prompt helper is a shared primitive any future command can opt into.
