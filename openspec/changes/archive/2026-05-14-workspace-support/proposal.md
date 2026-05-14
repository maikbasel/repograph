## Why

Phase 1 (`registry-core`) gave us a flat registry of repositories — every repo lives in one undifferentiated list, with no way to scope a query to "the repos that make up project X". The whole point of `repograph` is to feed AI agents structured cross-repo context, and an agent that asks for context on the "Acme rebuild" doesn't want every repo on disk — it wants the three repos that matter for that initiative. A developer with thirty repos registered has the same need at the human side: `repograph list` is already too noisy to be useful, and `status`/`context` (Phases 3 and 4) will be unusable without a filter. Workspaces are the layer that makes the registry navigable for both audiences before downstream commands ship.

## What Changes

- **NEW**: `repograph workspace create <name> [--description <text>]` — define an empty named grouping. Enforces RFC 1123 label-style naming rules (`^[a-z0-9][a-z0-9-]{0,62}$`, reserved words `default`/`all`/`none` rejected) at write time.
- **NEW**: `repograph workspace rm <name>` — delete a workspace. Does not touch registered repos themselves.
- **NEW**: `repograph workspace ls` — list workspaces. TTY = `comfy-table`, non-TTY / `--json` = `{ "workspaces": [...] }` envelope.
- **NEW**: `repograph workspace show <name>` — show one workspace's members (resolved against the registry). JSON envelope includes a `dangling: []` array surfacing tombstoned members so agent consumers can detect drift without a separate command.
- **NEW**: `repograph workspace add <workspace> <repo> [<repo>...]` — attach one or more registered repos to a workspace. Idempotent on duplicate adds; atomic on missing-repo failures.
- **NEW**: `repograph workspace remove <workspace> <repo> [<repo>...]` — detach members. Idempotent on non-members.
- **NEW**: `--workspace <name>` filter on `repograph list` — flag-only filtering, no persistent active-workspace state. Existing `list` behavior unchanged when the flag is omitted.
- **NEW**: TOML schema additions — `[workspace.<name>]` table with `description: Option<String>` and `members: Vec<String>` (sorted on write). Sits alongside the existing `[repo.<name>]` entries in the same `config.toml`.
- **NEW**: Tombstone semantics — when a registered repo is removed, its name is left intact in any workspace `members` arrays it appeared in. Workspace read paths surface dangling members via a stderr warning (TTY) and the `dangling` JSON field (machine-readable). Full cleanup is deferred to Phase 5's `doctor` command.
- **UNCHANGED**: `registry-core` behaviors — `add`/`list`/`remove` keep their archived contract intact. `remove` does NOT learn about workspaces; tombstone semantics keep the registry pure.

## Capabilities

### New Capabilities

- `workspace-support`: Named groupings of registered repositories, CRUD over workspaces, membership management, tombstone semantics for dangling references, and a `--workspace` filter on `list`. Owns the `[workspace.<name>]` TOML schema and the `workspaces` / `dangling` JSON envelope shapes.

### Modified Capabilities

<!-- None. The registry-core spec is archived and stays invariant. Workspaces compose against it without amending it: members reference repo names, and tombstones avoid any change to registry-core's remove behavior. -->

## Impact

- **Code**:
  - `crates/repograph-core/src/config.rs` — new `Workspace` struct, workspace-keyed map on `Config`, `validate_workspace_name` helper, workspace CRUD methods, member add/remove with atomic multi-repo semantics, dangling-aware read accessors.
  - `crates/repograph-core/src/error.rs` — extends `RepographError` only if new variants are needed beyond the existing `NotFound` / `Conflict` / `Validation`-style variants (most cases reuse existing variants).
  - `crates/repograph/src/commands/workspace.rs` — new file, hosts the `Workspace` clap subcommand enum and `run()` dispatch for `create` / `rm` / `ls` / `show` / `add` / `remove`.
  - `crates/repograph/src/commands/list.rs` — gains an optional `--workspace <name>` argument and filter logic.
  - `crates/repograph/src/output.rs` — workspace table / JSON renderers added alongside the existing repo renderers.
  - `crates/repograph/src/main.rs` — wires the new `workspace` subcommand into the top-level dispatch.
  - `crates/repograph/tests/` — new acceptance tests covering each subcommand, the dangling tombstone flow, the `--workspace` list filter, and the JSON envelopes.
- **TOML schema**: additive only. `[repo.<name>]` entries are untouched; new `[workspace.<name>]` entries coexist. Round-trip stability (`save → load → save` byte-identical) extends to workspaces.
- **README.md**: command surface table, exit code table (no new codes — reuses 0/2/3/5), and example output blocks updated.
- **Dependencies**: no new crates. Reuses the existing `serde` / `toml` / `clap` / `comfy-table` / `tracing` / `thiserror` / `fs-err` stack.
- **Out of scope**: `workspace rename`, `workspace use` (persistent active context), multi-workspace filtering on `list`, glob/pattern membership, the `doctor` command, and effects on `status` / `context` (Phases 3 / 4 / 5).
