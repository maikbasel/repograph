## Why

`repograph` exists to feed AI agents structured context about a developer's local repositories, but every prior phase (`registry-core`, `workspace-support`, `git-status`, `init-command`) only built infrastructure. There is no command yet that produces the agent-facing payload itself. `init-command` shipped the `[agents]` selection and the agent-ID → file-pattern registry; the next step is the command that consumes them.

`repograph context` is the headline feature: given a scope (all repos, a workspace, or named repos), emit a single payload that inlines every registered repo's selected agent docs (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/*.md`, `.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.github/copilot-instructions.md`) ready to paste into a chat or pipe into a downstream agent.

## What Changes

- New `repograph context` subcommand that:
  - Resolves scope from one of `--workspace <name>`, positional `<repo>...` args, or (default) all registered repos
  - Reads `[agents] selected` from config (gated by `ensure_agents_configured` for first-use prompting in TTY, `NeedsInit` exit `2` otherwise — both shipped in `init-command`)
  - For each in-scope repo, walks the agent registry's file patterns against the repo root, reads the matching files, and emits them in the payload
  - Produces JSON on stdout when `--json` or stdout is not a TTY; Markdown when stdout is a TTY (both are stable, parseable data — Markdown is what an LLM eats)
- New `repograph-core::context` module exposing `Context` aggregation logic and a pattern-resolution helper that expands the agent registry's globs against a repo root (no glob magic outside this module)
- New `RepographError` variants for the new failure modes the command introduces (e.g. workspace-not-found surfaced through this command's `--workspace` arg)
- README updates: new command entry in the command table, example invocations for each scope mode, and an example of the JSON / Markdown payloads
- Tests cover: happy path (single repo, multi-repo, workspace scope), empty `[agents]`, missing repo on disk, unreadable agent file, binary file in glob match, glob with zero matches, malformed UTF-8, unknown repo / workspace name, large file body, output mode contract (stdout untouched by diagnostics, TTY vs non-TTY both produce parseable artifacts)

## Capabilities

### New Capabilities

- `context-command`: defines the `repograph context` subcommand surface, scope resolution semantics (default-all / `--workspace` / positional names), the agent-doc file resolution algorithm against the existing registry, the JSON payload shape (stable contract for downstream agents), the TTY Markdown rendering, the gating contract with `[agents]` configuration (no-op when present, prompt + persist in TTY, `NeedsInit` exit `2` otherwise), the per-repo and per-file error surface (warnings inline in the payload — never silent), and the exit-code mapping for scope failures.

### Modified Capabilities

_None._ `init-command` already owns the `[agents]` schema, the agent registry table, and `ensure_agents_configured`; `registry-core` already exposes `Config::repos()` / `Config::repo(name)`; `workspace-support` already exposes `Config::workspace(name)` and member accessors. `context-command` composes them — it does not change their contracts.

## Impact

- **Code**:
  - `crates/repograph-core/src/lib.rs` — register new `context` module
  - `crates/repograph-core/src/context.rs` — `Context`, `RepoContext`, `AgentDoc`, `MatchedFile`, plus `resolve_agent_docs(repo_path, agents) -> Vec<AgentDoc>` (the pattern-expansion engine)
  - `crates/repograph-core/src/error.rs` — new variants (e.g. `WorkspaceNotFound`, `RepoNotFound` if not already present from earlier phases; reuse otherwise)
  - `crates/repograph/src/commands/context.rs` — new command (Args, `run`)
  - `crates/repograph/src/commands/mod.rs` — register the subcommand
  - `crates/repograph/src/main.rs` — wire into the clap dispatch
  - `crates/repograph/src/output.rs` — new `render_context_markdown` / `render_context_json` helpers (TTY-aware)
  - `crates/repograph/Cargo.toml` — add `globset` (for glob matching against the registry patterns) if not already in core's deps; `walkdir` if directory walking is needed for the `.cursor/rules/*.md` pattern
- **Dependencies**: `globset` (new in `repograph-core`), possibly `walkdir` if glob expansion needs explicit recursive walking. No clap / cliclack churn — the command is non-interactive by default.
- **Public surface**: One new subcommand. Stable JSON payload becomes a contract for downstream agents — versioning considerations addressed in `design.md`.
- **Performance**: Per-repo file reads are I/O-bound; for N repos × M agent patterns, parallelize with `rayon` (already in core deps from `git-status`).
- **Docs**: README command table, exit codes (no new codes — reuse the contract), examples for each scope mode.
- **Not affected**: `Cargo.lock` (cargo manages), `release.yml`, `CHANGELOG.md`, `Cargo.toml` version (Release Please owns).
