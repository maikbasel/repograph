## Context

The repograph workspace exists as a Phase 1 skeleton: `repograph-core` exposes only a `VERSION` constant; the `repograph` binary prints a stub banner. The plan (`plan.md`) calls for `registry-core` as the next change — Phase 2. CLAUDE.md fixes the architectural shape (presentation/logic split, `OutputMode` pattern, exit-code contract, no shelling to git) and `.claude/rules/production-grade.md` forbids shipping anything half-done. This change establishes contracts that every later phase, including a future `repograph-mcp` binary that shares `repograph-core`, will inherit.

## Goals / Non-Goals

**Goals:**

- A working `add` / `list` / `remove` flow that round-trips through TOML on real disks, validated by `assert_cmd` acceptance tests against real `git2` repos in tempdirs.
- A `Config` / `Repo` model in `repograph-core` reusable by both the CLI and the future MCP server — domain types with no clap, no terminal output, no `println!`.
- The JSON envelope contract `{ "repos": [...] }` rendered by `output.rs` and asserted by tests.
- Full coverage of the exit-code contract (0/1/2/3/4/5) with each error path mapping deterministically to a code.
- TTY-aware rendering: `comfy-table` on terminals, JSON envelope when piped or `--json` is set; checked once at command entry, never re-checked inline.
- `tracing` for diagnostics on stderr, with command-level entry/success/error logs per `.claude/rules/logging.md`.

**Non-Goals:**

- Workspaces (deferred to `workspace-support`).
- Git introspection beyond "is this path a git repo?" — branch / dirty / ahead-behind are Phase 4 (`git-status`).
- Context aggregation, CLAUDE.md inlining (Phase 5, `context-command`).
- Shell integration, `doctor`, `--dry-run` (Phase 6, `shell-integration`).
- MCP server binary (separate, future phase).
- Migration tooling for the TOML schema — no version field is shipped, and all future additions will be additive via `#[serde(default)]`.

## Decisions

### D1. Repo identity: name (unique alias) + path (filesystem PK)

Both fields uniquely identify a `Repo`. Two checkouts of the same git repo at different paths register as **two distinct repos** (path is filesystem truth). Adding a repo with a name already in use, or a path already registered, is a conflict (exit 5).

**Alternatives considered:** name-only PK (rejected: silently masks duplicate path registrations); path-only PK (rejected: humans need a short alias for `workspace add --repos foo,bar`); a generated UUID (rejected: ceremony, no value when name is already a stable user-chosen alias).

### D2. Single TOML file, `[repo.<name>]` table layout, no version field

```toml
# ~/.config/repograph/config.toml  (or dirs::config_dir() equivalent)

[repo.changelog-x]
path = "/home/maik/IdeaProjects/changelog-x"
description = "Conventional-commits changelog generator"
stack = ["rust"]

[repo.repograph]
path = "/home/maik/IdeaProjects/repograph"
stack = ["rust"]
```

Workspaces (Phase 3) will land alongside as `[workspace.<name>]` tables in the same file. No top-level `version` field — additions are non-breaking iff every new field is `#[serde(default)]`. If we ever need a real migration, we add `version = 2` then.

**Alternatives considered:** separate `repos.toml` and `workspaces.toml` (rejected: the whole config fits in <50 KB even for power users; one atomic write is simpler); `[[repo]]` array of tables with a `name` field (rejected by user preference, table layout matches the way name will be used as a lookup key in workspaces).

**Trade noted:** with table layout, a CLI rename (future phase) deletes the old key and inserts a new one — the entry may visually reorder in the file. `toml::Table` preserves insertion order on serialize, so the JSON output and table rendering remain deterministic across writes.

### D3. JSON output shape — resource-keyed envelope

```json
{ "repos": [ { "name": "...", "path": "...", "description": "...", "stack": [...] }, ... ] }
```

Every command that returns a list uses an envelope keyed by the resource. The empty case is `{ "repos": [] }` (never `null`, never bare `[]`).

**Why envelope over bare array:** survey of established CLIs shows the split — `gh` returns bare arrays, `kubectl` always envelopes, `cargo metadata` envelopes with versioning. The MCP angle decides it: MCP tool responses are structured objects. With an envelope, the future `repograph-mcp` server hands back the same shape with one extra wrapper; with bare arrays, we'd carry two shape conventions for the same data and have to translate at the boundary. Cost in `jq`: `.repos[]` instead of `.[]` — six characters.

**Trade noted:** slightly less ergonomic for `jq` one-liners. Acceptable.

### D4. Test layout — Rust-standard three-tier

| Tier | Location | Purpose |
|------|----------|---------|
| Unit | `#[cfg(test)] mod tests` inline at the bottom of each `.rs` | private functions, narrow invariants (TOML round-trip, conflict detection) |
| Integration | `crates/repograph-core/tests/*.rs` | hits `repograph-core` public API with a real `tempdir` config root |
| Acceptance | `crates/repograph/tests/*.rs` via `assert_cmd` + `predicates` | drives the real binary, asserts stdout / stderr / exit code |

`crates/repograph/tests/common/mod.rs` (the `mod.rs` form intentional — prevents Cargo from compiling it as a test binary) holds shared helpers: `fixture_git_repo(tempdir)` initializes a real git repo via `git2`, `repograph_cmd_with_config(dir)` builds a `Command` pointed at an isolated config root via `XDG_CONFIG_HOME` (or platform equivalent) so tests never touch the user's real config.

**No mocking of `git2`** — per `.claude/rules/testing.md`, tests use real repos in tempdirs.

### D5. Outside-in TDD ordering

Acceptance tests for every scenario in `spec.md` are written **before** any of the core types exist. The first run shows them failing on missing types / unknown subcommands. Implementation then walks them green from the outside in: clap dispatch → command handler → core types → git2 adapter. Unit tests are added at the end only where they pin behavior an acceptance test can't observe cheaply (TOML byte-stability, name-vs-path conflict precedence).

### D6. Config-dir resolution: `--config-dir` > `REPOGRAPH_CONFIG_DIR` > platform default

The config directory is resolved with a three-layer precedence:

1. `--config-dir <PATH>` global CLI flag (highest priority)
2. `REPOGRAPH_CONFIG_DIR` environment variable
3. `dirs::config_dir().join("repograph")` — the platform default (Linux: `~/.config/repograph/`, macOS: `~/Library/Application Support/repograph/`, Windows: `%APPDATA%\repograph\`)

Implementation collapses (1) and (2) into a single clap annotation:

```rust
#[arg(long, global = true, env = "REPOGRAPH_CONFIG_DIR", value_name = "PATH")]
config_dir: Option<PathBuf>,
```

`global = true` makes the flag available on every subcommand (`add`, `list`, `remove`) without per-subcommand duplication. `env = "..."` makes clap fall back to the env var when the flag is absent. Resolution happens once at dispatch in `main.rs`; the resolved `PathBuf` is passed into `repograph-core` as a plain `&Path` argument. The core crate sees no env, no CLI types — just a directory.

If both the flag and the env var are unset *and* `dirs::config_dir()` returns `None` (rare — happens on platforms with no standard config home), the binary exits with code `1` and a stderr message instructing the user to pass `--config-dir`.

**Why a real flag over the env-only hook originally drafted:**

- Discoverable via `--help` instead of hidden in code.
- Unlocks a legitimate power-user pattern: separate registries per context (e.g. work vs. personal) by aliasing `repograph --config-dir ~/.config/repograph-work …`.
- Test isolation still falls out for free — tests set the env var on their `Command`, identical plumbing to before.
- The env var stays useful for `direnv`/CI/per-project setups where modifying every CLI invocation is awkward.

**Alternatives considered:** env-only (rejected — leaks a power-user feature behind an undocumented hook); flag-only (rejected — env vars are the natural fit for direnv/CI); per-subcommand flag without `global = true` (rejected — duplication, inconsistent surface).

**Naming:** `--config-dir`, not `--config` (which would be ambiguous: file vs. directory) or `--config-path` (the flag points at a directory, not a file).

### D7. Field set on `Repo`

```rust
pub struct Repo {
    pub path: PathBuf,
    pub description: Option<String>,
    pub stack: Vec<String>,  // serde default empty
}
```

Name is the map key in `Config { repos: BTreeMap<String, Repo> }`, not a field on `Repo` itself. `--stack` at the CLI accepts a comma-separated list and parses to `Vec<String>`. Empty `stack` serializes via `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. Same for `description`.

`BTreeMap` over `IndexMap`: deterministic alphabetical ordering for free; insertion order doesn't matter once we have a stable sort. JSON output sorts the same way.

### D8. Tracing initialization

Once in `main()`:

```rust
tracing_subscriber::fmt()
    .with_writer(std::io::stderr)
    .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
    .init();
```

Each command's `run()` is `#[tracing::instrument(skip(args))]` with `fields(...)` carrying key inputs. Entry → `debug`, success → `info`, error → `error` with `err = ?e`. No `println!` / `eprintln!` outside of `output.rs`'s explicit data renderers and `clap`'s own usage messages.

## Risks / Trade-offs

- **[Risk] TOML schema drift breaks user configs.** → Mitigation: every future field is `#[serde(default)]`; reads tolerate unknown fields (toml's default); writes preserve order; the test `config_roundtrip` pins byte-stability of a representative fixture.
- **[Risk] `git2`/libgit2 build failures across the five release platforms.** → Mitigation: `git2` defaults to vendored libgit2 on most targets; `cargo dist plan` (Phase 7) will surface any cross-compile issues before they hit users. Documented in README's install troubleshooting once we hit it.
- **[Risk] JSON shape locked too early.** → Mitigation: the envelope is forward-compatible (additive fields don't break clients). The risk is choosing the wrong field *names*. Mitigated by aligning names to the TOML keys (`name`, `path`, `description`, `stack`) — no synonyms, no abbreviations.
- **[Risk] Test isolation: a buggy test pollutes the real `~/.config/repograph/config.toml`.** → Mitigation: `REPOGRAPH_CONFIG_DIR` is set on every `Command` built by `repograph_cmd_with_config`; helpers refuse to construct a `Command` without it. CI runs in an empty `$HOME` anyway.
- **[Trade] `comfy-table` adds 30+ transitive deps.** → Accepted: hand-rolling a UTF-8 table renderer that handles unicode width correctly is brittle, and the alternative (no table, raw lines) defeats the TTY-mode design.
- **[Trade] `BTreeMap` orders alphabetically, not by insertion.** → Accepted: deterministic output is more valuable than insertion-order recall; users who want a custom order can re-add.

## Migration Plan

No prior version exists. First-write semantics:

1. `Config::load(dir)` — if file is absent, return `Config::default()` (empty registry); do not create the file.
2. `Config::save(dir)` — `fs::create_dir_all(dir)`, write atomically via temp-file + rename.
3. Malformed TOML on load → `RepographError::ConfigParse` → exit 1, stderr explains.
4. Permission denied on save → `RepographError::PermissionDenied` → exit 4.

Rollback: `repograph remove` is the inverse of `add`. No DB, no remote state — the file is the truth.

## Open Questions

- Do we want `repograph add` to canonicalize the input path (resolve symlinks, make absolute) before storing? **Tentative answer:** yes, store the canonical absolute path; surface relative paths back as absolute in `list`. Rationale: prevents two registrations of the same repo under different relative spellings. *(Will resolve during 4.2 implementation; if it changes, note as a resolved deviation.)*
- Should `--stack` accept repeats (`--stack rust --stack cli`) in addition to comma-separated? **Tentative answer:** yes — clap's `Vec<String>` with `value_delimiter = ','` supports both. Cheap, no design surface added.

## Resolved Deviations

- **Path canonicalization (open question 1) — resolved as drafted.** `validate_git_repo` calls `fs_err::canonicalize` before storing, so symlinks are resolved and relative paths are absolutized. Verified by acceptance test `path_stored_as_canonical_absolute`.

- **Unknown-field handling (D2 / spec scenario `Unknown fields are tolerated`) — partially resolved.** Loading tolerates unknown fields (verified by integration test `unknown_field_is_tolerated_on_load`). Preservation across save is *not* implemented: `serde`'s default `Deserialize` for our `Repo` struct drops unknown keys at the load boundary. The spec scenario explicitly allows either preservation or drop ("implementation choice documented in design"). We chose drop as the simpler, less surprising option; if a future schema needs additive forward-compat with preservation, that's a deliberate change to capture an `unknown_fields: BTreeMap<String, toml::Value>` flatten field.

- **Spec scenario `Platform has no default config dir and no override` — moved from acceptance to unit test.** On Linux/macOS, `dirs::config_dir()` falls back to `getpwuid` even when `HOME` and `XDG_CONFIG_HOME` are cleared, so the integration test cannot force the `None` branch through env manipulation. We refactored `resolve_config_dir(override, default)` to take both inputs explicitly; the scenario is verified by `main::tests::no_override_no_default_returns_usage_error_exit_1`. Behavior is unchanged from the spec.

- **Confirmation messages on stderr — converted from `eprintln!` to `tracing::info!`.** Initial draft used `eprintln!("registered '{name}'")` for user-facing confirmations, which violated the rule in `.claude/rules/logging.md` ("everything except `output.rs` data renderers goes through `tracing` to stderr"). The structured `tracing::info!(repo = %name, "registered")` event is emitted on stderr at the default `info` level and contains the repo name as a structured field — acceptance tests' `predicate::str::contains("foo")` assertions still pass.

- **`tracing::instrument` skip-args — required to avoid noisy auto-Debug output.** Without `skip(args)`, the instrument macro records the entire `Args` struct via `Debug`, producing log lines like `args=Args { name: "self" }`. Each command now uses `#[tracing::instrument(skip(args), fields(...))]` and explicitly extracts the relevant fields.
