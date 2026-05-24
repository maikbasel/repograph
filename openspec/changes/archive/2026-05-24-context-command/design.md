## Context

`repograph context` is the payoff command — the one users actually invoke to *get* context, after they've registered repos, grouped them, and declared their agent toolchain. Every prior change (`registry-core`, `workspace-support`, `git-status`, `init-command`) exists to make this command produce a meaningful payload.

The shape of the output is the contract: AI agents will paste it into chat sessions, pipe it through tooling, or read it as a file the editor surfaces. A flaky or unstable payload becomes silent context corruption in the calling agent, which is the worst class of bug per `.claude/rules/production-grade.md`.

**Current state**:

- `Config::repos()`, `Config::repo(name)`, `Config::workspace(name)`, `Workspace::members()` are stable from prior phases.
- `repograph-core::agents` already maps `AgentId` → `Vec<&'static str>` of file patterns (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/*.md`, `.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.github/copilot-instructions.md`).
- `repograph::prompt::ensure_agents_configured` already routes commands through the agent multiselect on first use (no-op when configured, prompt + persist in TTY, `RepographError::NeedsInit` exit `2` in non-TTY).
- The output-mode contract is settled: stdout = pure data (JSON or human-readable), stderr = diagnostics / spinners / warnings. `OutputMode` is computed once at the top of each command's `run()`.

**Constraints (from CLAUDE.md and rules/)**:

- No `unwrap`/`expect` outside test code; every failure mode maps to a `RepographError` variant with a documented exit code.
- `git2` only — even though `context` doesn't strictly need git history, the per-repo `branch` field comes through `git.rs`.
- Tests use `tempdir` + real `git2`; no mocks.
- `tracing` for diagnostics; stdout reserved for the payload.
- No `todo!()`, no half-finished implementations — both output modes ship; both are tested; both error paths are wired.

**Stakeholders**:

- AI agents consuming the JSON payload (primary; must be stable across patch releases).
- Developers eyeballing the Markdown payload in a TTY before pasting into a chat (secondary; aesthetics matter).
- The future `repograph-mcp` binary (planned, see CLAUDE.md), which will likely wrap `Context` aggregation as an MCP tool — so the core API surface needs to be reusable, not coupled to the CLI.

## Goals / Non-Goals

**Goals:**

- Produce one consolidated payload per invocation containing, for every in-scope repo, the inlined contents of every file matching the registry's patterns for the selected agents.
- Support three scope modes from one subcommand: default (all registered repos), `--workspace <name>` (members of a workspace), positional `<repo>...` (explicit list). Modes are mutually exclusive at the clap layer.
- Emit a stable JSON payload (versioned via a `schema_version` field) when `--json` or stdout is not a TTY.
- Emit a human-readable Markdown payload when stdout is a TTY — same data, different rendering.
- Surface per-file and per-repo errors *in the payload* (under `warnings` arrays), not by aborting the whole command. A missing `CLAUDE.md` in one repo must not stop the command from emitting the other six repos' docs.
- Reuse `ensure_agents_configured` so a first-time user invoking `context` gets prompted through agent selection (TTY) or a clear `NeedsInit` error (non-TTY), exactly like other agent-consuming commands.
- Parallelize per-repo file I/O via `rayon` — file reads across N repos are independent and add up fast on large workspaces.
- Stay within the existing exit-code contract (0/1/2/3/4/5). No new codes.

**Non-Goals:**

- **Token counting or budget enforcement.** Users / downstream tooling decide what fits in their context window. We don't truncate; we don't warn at thresholds. We log payload size on stderr for visibility.
- **File watching / regeneration.** One-shot command; no daemon, no `--watch`.
- **Output-to-file flag.** `repograph context > ctx.md` already works; a `--output` flag adds surface area for no benefit.
- **Caching across invocations.** Files are re-read every call. Cheap, simple, no staleness bugs.
- **Filtering individual agents at invocation time.** The `[agents]` selection from config is the source of truth. A `--only <agent>` flag is YAGNI for v1 — users can re-run `repograph init` → `Update agent selection` if they need to slim the output.
- **Filtering individual files within an agent's pattern set.** Out of v1 scope.
- **Embedding git status in the payload.** `repograph status` exists; users can run both. Including only `branch` here gives the agent enough orientation without coupling to the full status struct or paying the `git2::Statuses` cost on a command meant to be quick.
- **Pretty-printed JSON by default.** JSON output is single-line (or jq-friendly) for stable pipe handling; a `--pretty` flag may come later.
- **Network operations.** `git fetch`, remote tracking refresh, etc. are status-command concerns.
- **Binary / non-UTF-8 file content.** Reported as a warning entry per file; not base64-encoded into the payload.
- **Symlinks pointing outside the repo root.** Followed if `globset` follows them by default; not specially handled.

## Decisions

### Decision 1: Glob engine — `globset` over `glob`

**Choice**: Add `globset` to `repograph-core` for compiling the agent registry's patterns and matching them against files within a repo root.

**Rationale**:

- `globset` is the engine `ripgrep` uses; battle-tested, fast at compiling and matching many globs against many paths.
- It supports `GlobSet` (compile once, match many) which fits our shape: compile the patterns for the user's `[agents]` once, then check each candidate file in each in-scope repo.
- The `glob` crate is path-walking-oriented; we have a fixed root per repo and known patterns. `globset` is the better fit and integrates with `walkdir` if we end up recursing for `.cursor/rules/*.md`.

**Alternatives considered**:

- **`glob` crate**: simpler API for single-pattern walks, but slower for many patterns and requires us to walk the filesystem per pattern. Rejected.
- **Hand-rolled matching**: the registry has only seven patterns total today, but the moment any one of them gains a `**` or `*` segment it gets fiddly. Rejected — globbing is a solved problem.
- **`ignore` crate**: ripgrep's higher-level walker. Overkill — it pulls in `.gitignore` semantics we don't want here. Rejected.

### Decision 2: Pattern expansion is owned by `repograph-core::context`, not `agents`

**Choice**: `agents.rs` stays a pure registry (ID → patterns table). The new `context.rs` module owns `resolve_agent_docs(repo_root: &Path, agents: &[AgentId]) -> Vec<AgentDoc>`, which compiles the patterns into a `GlobSet` and walks the repo root looking for matches.

**Rationale**:

- `agents.rs` is intentionally tiny and table-shaped; mixing filesystem I/O into it breaks the "domain types are pure values" rule from `.claude/rules/logging.md`.
- `context.rs` is where the new logic lives; it depends on `agents.rs` but owns the filesystem walk.
- This keeps the `init-command` archive untouched — we add a new module rather than modifying a sealed one.

### Decision 3: Filesystem walk — bounded depth, scoped to known parent dirs

**Choice**: Don't walk the entire repo tree. The agent registry's patterns are either flat (`CLAUDE.md`, `AGENTS.md`, `.cursorrules`, etc.) or live in a known top-level dir (`.cursor/rules/*.md`, `.github/copilot-instructions.md`). For each pattern, infer the parent directory and read only that directory (non-recursive).

**Rationale**:

- Repos can have huge `node_modules` / `target/` trees. Walking them just to find `CLAUDE.md` at the root is wasteful and would invite us to add ignore-file logic to compensate.
- All seven v1 patterns have a known fixed prefix; we don't need a general recursive walker.
- For `.cursor/rules/*.md`, read `<repo>/.cursor/rules/` directly; for flat patterns, just `try_exists` the file.

**Alternatives considered**:

- **`walkdir` with depth limits**: works, but the depth depends on the pattern. More complex than reading known parent dirs directly. Reserved as fallback if future agents introduce deeper glob structures.
- **Full recursive walk with ignore-file support (`ignore` crate)**: explicitly out of scope. Repo-root agent docs are by convention at known locations.

### Decision 4: JSON payload schema — versioned, additive-only

**Choice**: The JSON payload includes a top-level `"schema_version": 1` field. Future additions to the schema are additive (new optional fields) at the same `schema_version`; breaking changes bump the version. Document the schema in the README's payload example, not in a separate JSON Schema file (overkill for v1).

**Rationale**:

- Downstream agents will key off the shape. A version field gives us a future escape hatch without forcing us to maintain a parallel schema file from day one.
- Additive-only at v1 means downstream parsers can be permissive — unknown fields are ignored, presence of known fields is the contract.

**Alternatives considered**:

- **No version field**: gambles that v1 is final. Cheap to add, expensive to retrofit. Rejected.
- **JSON Schema file in repo**: useful but premature. We have one consumer (agents); the README example is the contract for v1.

### Decision 5: Payload shape

**Choice**:

```json
{
  "schema_version": 1,
  "generated_at": "2026-05-24T14:23:11Z",
  "agents": ["claude-code", "cursor"],
  "scope": { "kind": "workspace", "name": "team-alpha" },
  "repos": [
    {
      "name": "api",
      "path": "/home/user/code/api",
      "branch": "main",
      "agent_docs": [
        {
          "agent": "claude-code",
          "files": [
            { "path": "CLAUDE.md", "bytes": 1234, "content": "..." }
          ]
        },
        {
          "agent": "cursor",
          "files": [
            { "path": ".cursor/rules/style.md", "bytes": 567, "content": "..." },
            { "path": ".cursorrules", "bytes": 89, "content": "..." }
          ]
        }
      ],
      "warnings": []
    }
  ],
  "warnings": []
}
```

- `scope.kind` ∈ `{ "all", "workspace", "repos" }`. `scope.name` present only for workspace; `scope.repos` present only for explicit lists.
- `path` is the canonical absolute path (same as everywhere else in the codebase).
- `branch` is the current branch name (or `null` for detached/unborn/bare/missing).
- `agent_docs[*].files[*].path` is **relative to the repo root** — agents resolve relative paths against the repo, and absolute paths leak local filesystem layout.
- `bytes` is the raw byte length of the file, useful for token-budget calculations downstream.
- `content` is the file's UTF-8 contents verbatim. No truncation.
- `warnings` arrays carry strings naming per-file or per-repo issues (`"failed to read CLAUDE.md: ..."`, `"path no longer exists"`).

**Rationale**:

- Flat structure matches what an LLM consumes: each repo block self-contained, agent groupings inside.
- Per-file `path` + `bytes` + `content` is the minimum any downstream tool needs (display, budget, content).
- Warnings inline keep the contract atomic: one invocation, one payload, every issue surfaced.

### Decision 6: Markdown rendering for TTY mode

**Choice**: TTY default renders Markdown to stdout in this shape:

```markdown
# repograph context — workspace `team-alpha` (2 repos, 2 agents)

## api  (branch: main)

`/home/user/code/api`

### claude-code

#### CLAUDE.md (1.2 KB)

```
<file contents>
```

### cursor

#### .cursor/rules/style.md (567 B)

```
<file contents>
```
```

**Rationale**:

- Markdown is data: it's what an LLM eats. The TTY user pasting into a chat gets a payload identical in informational content to the JSON one — just nicer to read.
- A human eyeballing the output in a TTY can see the structure (which agents matched, which files were inlined, sizes for budget intuition).
- Fenced code blocks preserve file content verbatim. We escape triple-backticks in file content by switching to `~~~` fences if the file itself contains triple-backticks (CLAUDE.md often does).
- Markdown is *not* a "preview" — it's the full payload. Users who want machine-parseable data pass `--json`.

**Alternatives considered**:

- **Table-only summary in TTY (file paths and sizes, no content)**: nicer-looking but breaks the "TTY default has the same data as JSON" promise. Rejected — `context` is for *getting* context, not previewing it.
- **Plain text with banners (no Markdown structure)**: harder to paste into chats that render Markdown. Rejected.
- **Always JSON**: violates the output contract that TTY = human-readable. Rejected.

### Decision 7: Scope mutual exclusion at the clap layer

**Choice**: `--workspace <name>` and positional `<repo>...` are mutually exclusive (clap `conflicts_with`). Specifying neither defaults to all registered repos.

**Rationale**:

- Mutual exclusion at the parse layer means the command body never has to handle "what if both are set?"
- The default (no flags) gives the user the easy on-ramp — `repograph context` Just Works against the whole registry.

### Decision 8: Parallelism — `rayon` per repo

**Choice**: After resolving the in-scope repo list, fan out across repos via `rayon::iter::IntoParallelIterator`. Each repo's work is independent: open the git repo (for the `branch` field), walk the agent patterns, read the files. Results collect into a `Vec<RepoContext>` in stable order (sort by name post-fan-out for deterministic output).

**Rationale**:

- File I/O dominates; serializing is wasteful on multi-repo workspaces (a 20-repo workspace × 7 patterns = 140 stat calls minimum, which serially is noticeable on cold caches).
- `rayon` is already a transitive dep from `git-status`; no new dep.
- Output order is stable by sorting on name post-collection, so JSON diffs are clean across invocations.

### Decision 9: Errors are inline warnings, not aborts

**Choice**: Per-file read failures, missing repo paths, and unreadable directories are recorded as `warnings` strings in the payload at the appropriate level (repo-level or top-level). The command exits `0` as long as the global setup succeeded (config loaded, agents configured, scope resolved).

The only failures that exit non-zero:

- Malformed config TOML → `1`
- `--no-prompt` / non-TTY with no `[agents]` configured → `NeedsInit` → `2`
- Both `--workspace` and `<repo>...` somehow set (defensive; clap should block) → `2`
- Specified workspace name not found → `3`
- Any specified repo name not found in registry → `3`
- Config write needed but permission denied (only possible via `ensure_agents_configured` path) → `4`

**Rationale**:

- Per the `production-grade.md` rule, every failure mode is designed. Partial-payload semantics (some repos failed, others succeeded) are the *right* semantics for a context command: agents downstream can still use the repos that succeeded.
- Aborting on the first missing file would make `context` brittle in real-world workspaces where repos drift (deleted, moved).

### Decision 10: Empty `[agents] selected = []` is valid, produces empty `agent_docs`

**Choice**: A user who completed `init` but selected zero agents (or deselected all) gets a payload with valid repo metadata and `agent_docs: []` per repo. No prompt, no error. The Markdown output renders the repo headers with a single line of italic text explaining no agents are selected.

**Rationale**:

- This is a configured-but-empty state per `init-command` spec. Treating it as "needs setup" would re-prompt every invocation, which the user has already declined.
- Useful for users who only want repo metadata (path / branch) as context — a thin payload is still a payload.

### Decision 11: File content read as UTF-8; non-UTF-8 surfaces as warning

**Choice**: Read every matched file with `fs_err::read_to_string`. On UTF-8 decode failure, emit a per-file warning (`"<path>: file is not valid UTF-8, skipped"`) and omit the file from `files`. Do not base64 / hex-encode binary content.

**Rationale**:

- Agent docs are by convention text (Markdown, YAML, plain rules). A binary `.cursorrules` is almost certainly a misconfiguration; we surface it loud rather than carry binary payload through to an LLM that can't read it.
- `fs_err` (already a project dep) gives us errors with path context for free.

### Decision 12: Tracing — log entry / count / size, never file bodies

**Choice**: Per `.claude/rules/logging.md`:

- `debug!(command = "context", scope = ?args.scope_kind, "start")` on entry.
- `info!(repos = repos.len(), agents = agents.len(), bytes = total_bytes, "context built")` on success.
- `warn!(repo = %name, file = %relpath, "non-UTF-8 file skipped")` for warning paths.
- `error!(err = ?e, "context failed")` on the error path.
- Never log file contents — log length and (if useful) a `blake3` digest. For v1, length-only.

**Rationale**: explicit in the logging rule; the auto-loaded rule is the contract.

## Risks / Trade-offs

- **[Risk] Payload size explodes on huge `CLAUDE.md` files** → Mitigation: log total bytes on `info`; document in README that `context` does not truncate; users pipe through `wc -c` or jq for inspection.
- **[Risk] JSON schema becomes unstable as we learn what downstream agents need** → Mitigation: `schema_version` field from v1; additive-only changes at v1; bump to v2 only for breaking changes; README documents the schema.
- **[Risk] `globset` brings in transitive deps and inflates binary size** → Mitigation: `globset` is small and already in the dependency closure for many Rust CLIs; acceptable cost for correctness. Verify via `cargo tree` after adding.
- **[Risk] Parallel I/O via `rayon` saturates a slow disk** → Mitigation: file I/O dominates and there's no good way to bound this short of a thread-pool config; if it becomes an issue, expose `--jobs N` later. For v1, default Rayon pool sizing is fine.
- **[Trade-off] No truncation of file bodies** → Means a single 10 MB rules file produces a 10 MB payload. Acceptable v1 trade-off; downstream tooling owns the budget. Documented in README.
- **[Trade-off] Markdown fence-collision handling (`~~~` fallback)** → Adds a small per-file scan for triple-backticks. Negligible cost; necessary for correctness when CLAUDE.md contains its own code blocks.
- **[Trade-off] No `--only <agent>` filter at invocation time** → Users wanting a slimmer payload must reconfigure via `init`. Acceptable v1 scope reduction; revisit if usage demands it.
- **[Risk] Non-TTY user without `[agents]` configured can't easily bootstrap** → Mitigation: `ensure_agents_configured`'s `NeedsInit` error already names `repograph init` on stderr per the init-command spec; no new work needed here.
- **[Risk] Workspace with members pointing at missing repos** → Mitigation: those repos render as `warnings: ["path no longer exists"]` entries; the payload is still emitted. Same pattern as `repograph status` per git-status spec.

## Migration Plan

No migration. This is a new command; no existing config is touched. The `[agents]` schema and `ensure_agents_configured` helper already shipped in `init-command` and are stable.

**Rollback strategy**: revert the change set; no on-disk state to clean up.

## Open Questions

_None at proposal time._ Open items will be tracked here as implementation surfaces them.

## Resolved deviations

Per `.claude/rules/documentation.md`, deviations from the original plan are recorded here rather than rewritten retroactively.

- **Time crate moved to the binary, not core.** The plan suggested adding `chrono` (or `time`) to `repograph-core`. The implementation puts `time = "0.3"` in the binary's `Cargo.toml` and stamps `generated_at` in `commands/context.rs::run` before constructing the `Context` envelope. Reason: core stays free of time dependencies, and the future `repograph-mcp` server gets the same option — generate the timestamp at its boundary, not buried in domain logic.
- **Parallelism lives in the binary, not core.** The plan put `rayon::par_iter` inside `Context::build` in core. The implementation instead exposes `RepoContext::build_one(name, path, agents)` as a per-repo primitive in core, and the binary fans out via the existing `output::with_progress` helper (which already wraps `rayon` and integrates with `indicatif::MultiProgress` for TTY spinners). Reason: no new dep in core, and we got TTY spinner integration for free with no extra code.
- **No `Context::build` aggregator in core.** Instead of a single core-side `Context::build(&Config, Scope) -> Result<Context>`, scope resolution (`resolve_targets`) + envelope construction live in `commands/context.rs`; core provides only the building blocks (`Scope`, `RepoContext::build_one`, `Context` struct, `SCHEMA_VERSION` const, `resolve_agent_docs` helper). Reason: lets the future MCP server compose those pieces with its own concurrency model and scope inputs without going through a CLI-shaped API.
- **No new `RepographError` variants for not-found.** The plan suggested adding `WorkspaceNotFound` and `RepoNotFound`. The existing `RepographError::NotFound { kind, name }` already handles both and is already mapped to exit `3`; reusing it kept the error surface flat.
- **`globset` lives in core; `serde_json` added to core's dev-deps.** Core now depends on `globset` (for pattern matching) and uses `serde_json` only in `#[cfg(test)]` to verify serialization shapes. Reason: pattern resolution is core domain logic; the serialization tests live next to the type definitions they verify.
