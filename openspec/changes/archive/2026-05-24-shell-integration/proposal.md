## Why

The first five changes (`registry-core`, `workspace-support`, `git-status`, `init-command`, `context-command`) shipped the full data path: register repos, group them, inspect git state, declare the agent toolchain, emit the agent payload. What is still missing is the connective tissue between repograph and the shell a developer actually lives in. There is no `repograph switch <name>` to teleport into a registered repo, no shell completions for any of the seven existing subcommands (so users tab into dead air), and no `repograph doctor` health check to surface the drift that accumulates over time (deleted paths, dangling workspace members, repos that lost their `.git/`, missing agent docs that quietly produce empty `context` sections).

These are the Phase 5 polish items called out in `CLAUDE.md`'s Manual Validation Checklist (`switch` exact stdout, `doctor` non-panicking) and in the dev-plan phase map (Phase 5 → `shell-integration`). They are the difference between "this binary works" and "this binary is pleasant to live with on the command line."

## What Changes

- New `repograph switch <name>` subcommand:
  - Resolves the named repo against the registry; emits exactly one line on stdout — `cd <canonical-absolute-path>` — and nothing else. Trailing newline is the only whitespace.
  - No `--json` mode (this command's stdout is executable shell, not data; honoring `--json` would break the eval contract). Non-TTY behavior is identical to TTY behavior — the stdout payload is shell-eval-safe in both modes.
  - Unknown repo exits `3` and writes nothing to stdout; the error message on stderr names the lookup and lists nearby repo names if any (Levenshtein distance ≤ 2).
  - README documents the companion shell snippets (bash/zsh `function rg-cd { eval "$(repograph switch "$1")"; }`, fish `function rg-cd; eval (repograph switch $argv[1]); end`).
- New `repograph completions <shell>` subcommand:
  - Generates static completion scripts for `bash`, `zsh`, `fish`, `powershell`, `elvish` via `clap_complete::generate` against the live `Cli` struct (so the completions never drift from the command surface).
  - Writes to stdout for one-time install (`repograph completions fish > ~/.config/fish/completions/repograph.fish`); stderr stays empty on success.
  - Unknown shell value rejected by clap (exit `2`); supported shells are the full `clap_complete::Shell` set.
- New `repograph doctor` subcommand:
  - Read-only health check that loads the config and runs a fixed battery of checks across every registered repo and workspace, plus the agent configuration.
  - Per-check result is one of `ok`, `warn`, `error`. Findings are reported in TTY mode as a `comfy-table` summary (one row per finding) plus a final `N ok · M warn · K error` tally; `--json` (or non-TTY) emits a `{ "schema_version": 1, "checks": [...], "summary": {...} }` envelope.
  - Exit codes: `0` when every check is `ok` or `warn` only (i.e. no `error` findings); `1` when any check is `error`; `4` when the config file cannot be read due to permission denied. A missing config file is reported as a single error finding and exits `1`, not `3` (this command is diagnostic — it always reports, never crashes on absence).
- New `crates/repograph-core/src/doctor.rs` module:
  - Owns the `Finding` type (`Severity` enum, `Check` enum naming the check, `target` string, `message`), the `DoctorReport` aggregate, and the pure check functions (config-shape checks, repo-path existence + git-repo validity via `git2`, workspace dangling members, agent-doc presence per repo for the configured `[agents].selected`).
  - No I/O lives in the binary's command file beyond delegating to core and rendering the report.
- New shared CLI primitive `crates/repograph/src/main.rs` → `Cli` struct exposed to the `completions` command via a small helper so `clap_complete` can introspect it without re-declaring the structure.
- README updates: new command table entries for `switch`, `completions`, `doctor`; a "Shell integration" subsection documenting the `rg-cd` snippets and one-time completion install per shell; a "Doctor" subsection showing the JSON envelope shape and the check catalog.
- New `repograph-core` dep: none (uses existing `git2`, `serde`, `thiserror`, `fs_err`). New `repograph` (binary) dep: `clap_complete` for the `completions` command.
- Tests cover: `switch` happy path (stdout is exactly `cd <path>\n`, no banner / no log line leaks into stdout), unknown repo exit `3` + suggestion hint, multiple repos with one matching name; `completions` for each supported shell (stdout contains a shell-specific marker line — e.g. `complete -c repograph` for fish, `_repograph()` for bash); `doctor` happy-path (all green, exit `0`), each finding category (missing path, non-git path, dangling workspace member, missing agent doc, missing `[agents]` section), JSON envelope shape, `--json` vs TTY parity (same findings, different rendering), permission-denied on config → exit `4`.

## Capabilities

### New Capabilities

- `shell-integration`: defines the `repograph switch <name>` subcommand surface (resolution against the registry, the exact `cd <path>` stdout contract, the no-`--json` rule, the unknown-name exit and suggestion behavior, the documented companion shell snippets), and the `repograph completions <shell>` subcommand (the supported shell set, the stdout-only output, the live introspection of `Cli` so completions never drift). Both subcommands compose `registry-core` and the existing `Cli` parser without modifying them.
- `doctor-command`: defines the `repograph doctor` subcommand — the full check catalog (config-shape, per-repo path existence + git validity, workspace dangling members, agent-doc presence per `[agents].selected` × in-scope repos, `[settings].projects_root` existence when present), the `Finding` / `Severity` / `Check` data model, the TTY summary table, the stable `--json` / non-TTY envelope shape (versioned via `schema_version`), the exit-code mapping (`0` clean / `0` warn-only / `1` any error / `4` config-unreadable), and the read-only contract (no config writes, no `git fetch`, no network).

### Modified Capabilities

_None._ `registry-core` already exposes `Config::repos()` and `Config::repo(name)`; `workspace-support` already exposes the workspace lookup helpers and dangling-member surface; `init-command` already owns the `[agents]` schema; `context-command` already owns the agent-doc pattern resolver. `shell-integration` and `doctor-command` compose these primitives — they do not change their contracts.

## Impact

- **Code**:
  - `crates/repograph-core/src/doctor.rs` — new: `Finding`, `Severity`, `Check`, `DoctorReport`, `DoctorReport::run(&Config) -> DoctorReport` plus the per-check helpers. Uses existing `git.rs` for git-repo validity and existing `context::resolve_agent_docs` for agent-doc presence (no duplication).
  - `crates/repograph-core/src/lib.rs` — register the `doctor` module and re-export `DoctorReport`, `Finding`, `Severity`, `Check`.
  - `crates/repograph/src/commands/switch.rs` — new: `Args { name: String }` + `run(args, &config_dir) -> Result<(), RepographError>`. Stdout is the bare `cd <path>` line; nothing else.
  - `crates/repograph/src/commands/completions.rs` — new: `Args { shell: clap_complete::Shell }` + `run(args) -> Result<(), RepographError>` that invokes `clap_complete::generate` against the live `Cli` (helper that returns `<Cli as clap::CommandFactory>::command()`).
  - `crates/repograph/src/commands/doctor.rs` — new: `Args { json: bool }` + `run(args, &config_dir) -> Result<(), RepographError>`. Delegates to `repograph_core::DoctorReport::run` and dispatches to `output::render_doctor_{table,json}`.
  - `crates/repograph/src/commands/mod.rs` — register `switch`, `completions`, `doctor`.
  - `crates/repograph/src/main.rs` — wire three new `Command` variants and dispatch arms; expose the `Cli` type to the `completions` module via a small factory helper.
  - `crates/repograph/src/output.rs` — new `render_doctor_table` and `render_doctor_json` helpers (TTY-aware); the `switch` command writes its single line directly without going through `output.rs` (the line is not "data" in the table/JSON sense — it's an executable shell snippet).
  - `crates/repograph/Cargo.toml` — add `clap_complete` (binary crate only; core stays terminal-free).
- **Dependencies**: one new binary-only dep (`clap_complete`). No new core deps.
- **Public surface**: three new subcommands. The `doctor` JSON envelope becomes a contract for downstream consumers (CI health gates, agents that introspect repograph's state) — versioned via `schema_version: 1` from day one, same pattern as `context-command`. The `switch` stdout shape is a hard contract: exactly `cd <absolute-path>\n` with no variation across modes.
- **Exit codes**: reuses the existing contract. `switch` uses `0`/`3`; `completions` uses `0`/`2` (clap rejects unknown shell); `doctor` uses `0`/`1`/`4`. No new codes added to the table in CLAUDE.md or README.
- **Performance**: `doctor`'s per-repo checks are I/O-bound; reuse the existing `rayon` parallelism pattern from `context-command` (per-repo fan-out, collect into a stable-sorted `Vec<Finding>`). `switch` and `completions` are O(1).
- **Docs**: README gains command table rows for the three subcommands, a "Shell integration" section with the `rg-cd` snippets for bash / zsh / fish + completion install one-liners, and a "Doctor" section with the JSON envelope example and check catalog. CLAUDE.md Manual Validation Checklist is already in shape — `switch` and `doctor` move from "TBD" to documented behavior here.
- **Not affected**: `Cargo.lock` (cargo manages), `.github/workflows/release.yml` (cargo-dist owns), `CHANGELOG.md` (Release Please owns), `Cargo.toml` `version` field (Release Please owns).
