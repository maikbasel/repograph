## Why

The Phase 1 skeleton ships a binary that does nothing useful — `repograph` prints a stub banner and exits. Every later phase (workspaces, status, context, MCP server) operates over a *registered set of repos*. Until users can `add`, `list`, and `remove` repos with persistence, none of the planned features have anything to act on. `registry-core` is the first change that makes the binary do real work and locks in the contracts that downstream phases inherit: the TOML schema, the JSON output shape, the exit-code map, and the stdout/stderr separation.

## What Changes

- Add `repograph add <path> [--name] [--description] [--stack]` — registers a local git repository (validated via `git2::Repository::open`) into the user's config.
- Add `repograph list [--json]` — renders the registry as a `comfy-table` on TTY, JSON envelope when piped or `--json` is set.
- Add `repograph remove <name>` — deregisters a repo by name.
- Introduce `Config`, `Repo` domain types in `repograph-core` with TOML persistence at `dirs::config_dir()/repograph/config.toml`.
- Introduce `RepographError` (`thiserror`) with full exit-code mapping per the contract in `CLAUDE.md` (0 / 1 / 2 / 3 / 4 / 5).
- Introduce `OutputMode` in the CLI binary; TTY detected once at command entry via `is-terminal`.
- Initialize `tracing-subscriber` in `main`, writing structured diagnostics to stderr.
- Lock the JSON output shape: resource-keyed envelope `{ "repos": [...] }`, no version field, no metadata block.
- Lock the TOML schema: single config file, `[repo.<name>]` table layout, no version field, all future fields additive via `#[serde(default)]`.

## Capabilities

### New Capabilities

- `registry-core`: persistent registry of local git repositories — add / list / remove with TOML-backed config, the JSON envelope contract, the TTY-aware output mode, and the exit-code map. Every later capability composes against this one.

### Modified Capabilities

None — this is the first capability.

## Impact

- **Code (new)**: `crates/repograph-core/src/{config,git,error}.rs`; `crates/repograph/src/{output,commands/{add,list,remove}}.rs`; `crates/repograph/tests/{common,add,list,remove,output_contract}.rs`; `crates/repograph-core/tests/config_roundtrip.rs`.
- **Code (modified)**: `crates/repograph/src/main.rs` (tracing init, clap dispatch, exit-code mapping).
- **Dependencies (runtime)**: `clap` (derive), `serde`, `serde_json`, `toml`, `git2`, `thiserror`, `dirs`, `fs-err`, `tracing`, `tracing-subscriber`, `is-terminal`, `comfy-table`.
- **Dependencies (dev)**: `assert_cmd`, `predicates`, `tempfile`.
- **User-visible**: creates `~/.config/repograph/config.toml` on first write (or platform equivalent via `dirs::config_dir()`); writes a sample TOML structure documented in README.
- **Contracts locked**: JSON envelope shape, TOML key layout, exit-code map, stdout-data / stderr-diagnostics separation. Future changes inherit these.
- **Out of scope**: workspaces (Phase 3), git status (Phase 4), context aggregation (Phase 5), shell integration & doctor (Phase 6), distribution (Phase 7), MCP server.
