## Context

repograph ships two passive integration mechanisms: per-agent instruction artifacts (`agent_artifact.rs`, six agents) and a `POINTER` block spliced into a repo's `CLAUDE.md` / `AGENTS.md`. Both are well-formed. Neither is invoked.

The observed behaviour is that agents resolve "which repo was that in?" with `Grep` / `find` rather than `repograph`. The mechanism is a cost asymmetry at the point of tool selection: `Grep` is one call with a known cost, while repograph requires recognising the situation, recalling that a skill exists, invoking `Skill`, reading its body, and only then shelling out via `Bash`. The payoff is unknown until the fifth step. Nothing in the skill's wording changes that ordering.

Research into how comparable tools solve this (CodeGraph, Supabase, GitHub, Prisma, Netlify, `microsoft/skills`) found no single canonical answer, but a clean split. Cloud products migrated to **remote** HTTP MCP — structurally unavailable to repograph, which reads the local filesystem. Among tools that must run locally, the pattern is a **stdio MCP server as a subcommand of the existing binary**. CodeGraph, already installed on the maintainer's machine as `codegraph serve --mcp` (`type=stdio`), is the nearest neighbour: one binary, an `install` command that registers it, and no skills at all.

There is a credible dissenting position — that skills plus a plain CLI beat MCP because tool schemas are paid on every turn. Its argument rests on models knowing tools like `git` and `docker` cold from training data, and it explicitly concedes MCP for the case where "an agent is meeting a new integration for the first time and has no prior training exposure to it." That is repograph exactly. The dissent's own exception selects MCP here, but its token warning is real and shapes the size of the surface below.

**Prior decision superseded.** D7 of `2026-05-28-agent-skills` cancelled a planned `repograph-mcp` binary, costing it at "a second binary, a second distribution channel, an MCP runtime, and clients that speak MCP." A subcommand is not a second binary and needs no second channel; `tokio` is already a binary dependency; and all four MCP-hosting agents in the registry now speak the protocol. Three of four premises have expired. D7 was correct on the evidence available in May.

## Goals / Non-Goals

**Goals:**

- Put repograph's read surface into the agent's tool list so tool selection, not recall, is the deciding step.
- Ship it on the existing binary — no new crate, no new cargo-dist target, no new distribution channel.
- Hold the per-turn schema cost to roughly 600–1200 tokens.
- Have `repograph init` register the server through each vendor's documented mechanism, and `repograph doctor` verify it.
- Keep `repograph-core` synchronous and free of protocol concerns.
- Leave every existing CLI surface, JSON envelope, and exit code untouched.

**Non-Goals:**

- Exposing mutating operations (`add`, `edit`, `remove`, `workspace …`) over MCP.
- A remote/HTTP transport. repograph reads the local filesystem; there is nothing to host.
- A Claude Code plugin. It serves one of six agents and adds a marketplace repo to maintain; revisit only if users ask for a one-line install.
- A SessionStart hook injecting the repo roster. MCP subsumes what it would buy, and writing into `settings.json` is not repograph's to do.
- Replacing the skills. Skills and MCP are complements with different jobs (see D6).

## Decisions

### D1: stdio MCP as `repograph mcp serve`, on the existing binary

`repograph mcp serve` speaks JSON-RPC over stdin/stdout. Clients spawn it as a child process; there is no port, no daemon, no network.

*Alternatives considered.* A separate `repograph-mcp` binary (D7's framing) doubles the cargo-dist matrix from five targets to ten, adds a second crates.io publish and a second Homebrew formula, and creates a version-skew surface between binary and server. A remote server is impossible — the registry and the repos are local. Requiring `npx`-style out-of-band installation is unavailable to a Rust binary.

The `mcp` subcommand is a parent with `serve` beneath it rather than a bare `repograph mcp`, leaving room for `repograph mcp register` / `repograph mcp status` later without a breaking rename.

### D2: exactly six read-only tools, `readOnlyHint: true`

`repograph_list`, `repograph_status`, `repograph_context`, `repograph_switch`, `repograph_find`, `repograph_doctor`.

Two forces set this number. Schema tokens are paid on every turn, and the failure mode the dissent warns about is a 50+ tool server burning tens of thousands of tokens before answering anything. Against that, the tool must be *there* to be chosen. Six tools with tight descriptions is the smallest surface that covers every read intent the skills currently describe.

`readOnlyHint: true` matters more than it looks: clients can auto-approve read-only tools, and a tool that raises a permission prompt on every call gets avoided by agents and humans alike. That is a second, quieter suppressor of adoption beyond recall, and it is fixed here rather than left for later.

*Alternative considered.* Exposing all commands including mutations. Rejected: it roughly doubles schema cost, forfeits blanket `readOnlyHint`, and contradicts the existing skill split where the registry stays the user's to manage behind `repograph-setup`'s plan→confirm→execute flow.

### D3: MCP lives in the binary crate, as a sibling of `output.rs`

New module `crates/repograph/src/mcp/` (tool definitions, dispatch, error mapping) plus `commands/mcp.rs` for the subcommand.

CLAUDE.md's boundary puts domain logic in `repograph-core` and presentation in the binary. MCP is a wire format — the same category as the JSON rendering already in `output.rs`, not a new domain concept. Placing it in core would drag `rmcp` and an async runtime into a library that is published separately and is deliberately synchronous and dependency-light.

### D4: tools call core APIs directly; they do not wrap command `run()`

Command handlers mix data production with rendering and are `Result<(), RepographError>` — `list.rs` calls `Config::load` then `render_repos`; there is no data value to intercept. MCP tools therefore call the same core APIs the handlers call (`Config::load`, `resolve_workspace`, `search`, `refresh_stale`, `DoctorReport::run`) and serialise the result themselves.

This makes MCP tools a **sibling adapter** to `commands/`, not a layer above it, and requires no refactor of existing handlers. The cost is that the JSON shape is produced in two places, which D5 addresses.

`repograph_find` inherits the `auto-index-refresh` behaviour for free by calling `refresh_stale` exactly as `find.rs` does, including the `no_refresh` escape as an optional parameter. Index freshness semantics stay identical across CLI and MCP by construction.

### D5: MCP payloads reuse the documented JSON envelopes verbatim

Each tool returns the same JSON the corresponding `--json` invocation produces — same keys, same `schema_version` where present. The serde types in core are the single source of truth; the MCP layer adds no shapes of its own.

This keeps one contract to document, test, and version. Where a command's envelope is produced by binary-side rendering rather than a core serde type, that shape moves into a shared serialisable type so both paths derive from one definition — the targeted refactor that keeps D4's duplication from becoming drift.

`repograph_switch` is the one deliberate divergence: the CLI prints `cd <quoted-path>` because its consumer is a shell. The MCP tool returns the resolved path as data. Emitting a shell command to a JSON-RPC client would be a category error.

### D6: skills are retargeted from mechanics to policy, not deleted

`agent_artifact_body.md` is today largely a command table and a JSON-envelope reference. Typed tool schemas carry both. What survives is what MCP cannot express: prefer repograph over `Grep` for cross-repo questions; do not use it for the current directory's own git status; do not loop when one `context` call covers every repo.

CodeGraph is the counter-example that justifies keeping them: it dropped skills entirely, and the preference rule had to be hand-written into the user's own global `CLAUDE.md` for it to fire. repograph ships that rule itself. Skills cost roughly 30 tokens idle via progressive disclosure, so the retained policy is close to free.

`ARTIFACT_BODY_VERSION` increments so `refresh_installed_artifacts` rewrites managed sections in place; the existing delimiter contract already makes this idempotent.

Aider and `AGENTS.md` have no MCP host and keep the fuller body, since for them the CLI mechanics are still the whole story. This makes the body non-uniform for the first time — see D7.

### D7: registration goes through each vendor's documented mechanism

| Agent | Mechanism |
|---|---|
| Claude Code | `claude mcp add repograph -- repograph mcp serve` when the `claude` CLI is present; otherwise write `.mcp.json` (project scope) |
| Cursor | `~/.cursor/mcp.json`, or `.cursor/mcp.json` at project scope |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` |
| Copilot | `.vscode/mcp.json` |
| Aider, AGENTS.md | no MCP host — artifact only, unchanged |

Preferring a vendor's own CLI over hand-writing its config is the accepted convention; hand-editing is the documented fallback where no CLI exists. Registration is idempotent and additive — it merges a `repograph` key into an existing `mcpServers` map and never rewrites unrelated entries, mirroring the delimiter discipline `splice_managed_section` already applies to markdown.

The binary path recorded in the registration is the resolved absolute path of the running executable, not the bare name `repograph`, so a client spawning it does not depend on `PATH` inheritance.

### D8: stdout belongs to the protocol in `serve` mode

For every other command, stdout is data and stderr is diagnostics. Under `mcp serve`, stdout **is** the JSON-RPC frame stream: a single stray write corrupts the session. The existing `tracing`-to-stderr discipline becomes load-bearing rather than conventional, and `serve` must install a subscriber that cannot write to stdout and suppress every progress/spinner path (`indicatif` included) that a shared code path might otherwise trigger.

### D9: reuse the existing tokio runtime pattern

`commands/update.rs` already drives async code on a private current-thread runtime, with `tokio` at `default-features = false, features = ["rt", "net", "time"]`. `serve` follows the same pattern, adding only the `io-std` feature for stdin/stdout transport. No workspace-wide async migration, no `#[tokio::main]`, and `main()` stays synchronous.

## Risks / Trade-offs

- **Schema tokens are paid on every turn, whether or not repograph is used** → Mitigated by the six-tool cap (D2) and terse descriptions. Budget is asserted in a test that fails if the serialised schema exceeds its ceiling, so growth is a deliberate decision rather than a drift.
- **JSON shapes now produced in two places (D4)** → Mitigated by D5: shared serde types are the single source, and tests assert MCP tool output is byte-identical to the corresponding `--json` output.
- **A stray stdout write corrupts an MCP session (D8)** → Mitigated by an integration test that drives a real `serve` process over a pipe and asserts every stdout byte parses as JSON-RPC, including on error paths and with a repo whose path no longer exists.
- **Vendor config formats drift** → Registration is a closed per-agent mapping, same shape as `file_patterns()`; a format change is a one-arm edit. `doctor` surfaces a registration that no longer resolves rather than failing silently.
- **`serve` is a long-lived process, unlike every other subcommand** → It holds no lock and opens the SQLite index per call, so a crash loses nothing; the client restarts it. Config is re-read per call so registry edits are picked up without a restart.
- **`find` latency inside an agent loop** → `refresh_stale` is mtime-gated (a stat sweep in the common case) and the `no_refresh` parameter escapes it entirely.
- **Non-uniform artifact bodies across agents (D6)** → First divergence in a body that has been shared since inception; the alternative is telling Aider users about tools they cannot call. Tracked by a test that pins which agents get which body.
- **Adoption may still fall short** → MCP raises the probability substantially but guarantees nothing; CodeGraph needed both a server *and* a preference rule to fire reliably, which is why D6 keeps the skill. If invocation is still low after this ships, the next lever is the roster-injection hook, deliberately deferred here.

### D10: existing installs reconcile themselves after an upgrade

An install that predates this change has no MCP registration and a stale artifact body. Requiring users to re-run `repograph init` to get either would mean most existing users never get them — the whole point of the change would land only for new installs.

Repograph ships through four channels. Only one of them (`repograph update`) runs repograph code during the upgrade; Homebrew, `cargo install`, and the shell installer all replace the binary behind its back. So hooking the upgrade itself covers a minority of users. The reconciliation must instead be **triggered by the new binary noticing it is new**.

`[settings]` gains a `setup_version` stamp recording the `repograph` version that last completed setup. After any successful command, if agents are configured and the stamp differs from the running version, repograph re-runs the idempotent parts of setup for the agents *already selected*: refresh the managed artifact sections, and register the MCP server. It then writes the new stamp, so the work happens once per upgrade rather than once per command.

Three constraints keep this from being invasive:

- **It never makes a new decision.** It only repairs what `init` already established — no new agents, no prompting, no scope changes. A user who never ran `init` has no `[agents]` section and is left completely alone.
- **It only touches managed regions.** Artifacts go through the existing delimiter contract; registration merges one key into an existing server map. Both are already idempotent.
- **It cannot break the command that triggered it.** Every failure is caught and logged to stderr, exactly as `selfupdate::notify` is gated today. It runs after the command's own work, so it can never delay or corrupt output.

*Alternative considered.* Running reconciliation inside `repograph update` only. Rejected: it serves one of four install channels, and the three it misses include both package-manager paths.

*Alternative considered.* Prompting on first run after an upgrade. Rejected: the common case is a non-interactive agent session, where a prompt is either invisible or a hang.

### D11: registration writes vendor config directly, for every agent

Each MCP-hosting client keeps its server map in a JSON config file, and repograph merges one key into it, preserving everything else:

| Agent | User scope | Project scope |
|---|---|---|
| Claude Code | `~/.claude.json` | `.mcp.json` |
| Cursor | `~/.cursor/mcp.json` | `.cursor/mcp.json` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` | same |
| Copilot | — | `.vscode/mcp.json` (`servers` key, not `mcpServers`) |

*Alternative considered and rejected during implementation:* preferring `claude mcp add` for Claude Code, on the reasoning that letting a vendor manage its own large config file is safer than rewriting it. This was the original decision and it was wrong for a reason that only surfaced under test — see **R7**.

## Resolved deviations

Recorded during implementation; the design above states intent, these state what was actually built and why it differs.

**R1 — Config is loaded per call, so a malformed config is a tool error, not a startup failure.** The spec originally required `mcp serve` to exit with the documented code when started against a malformed config. Implementation chose to load config inside each tool call instead, which means the server starts successfully and the parse failure surfaces on first use. The lazy read is worth more than the eager validation: it lets `repograph add` take effect without restarting every connected client, and a client that spawned the server has no good way to surface a startup exit code to the user anyway — whereas a tool error is rendered in the conversation. The spec scenario was updated to match.

**R2 — No serve-specific tracing subscriber was needed.** D8 called for installing one. `init_tracing` in `main.rs` already pins `.with_writer(std::io::stderr)` globally, and no MCP tool reaches a `with_progress`/`indicatif` path — the tools call core APIs directly and pass a `tracing::debug` closure where `refresh_stale` wants a progress callback. The guarantee is asserted by test rather than established by new code: `serve_session` parses every stdout line as JSON-RPC, so any leak fails the whole MCP suite.

**R3 — `repograph_context` refuses to run without a configured agent selection.** Not anticipated in the design. The CLI's `context` calls `ensure_agents_configured` and exits 2 when the selection is empty; the first parity test caught the MCP tool happily returning an empty `agent_docs` instead. Returning empty would tell the caller "these repos have no conventions" when the truth is "repograph was never set up" — a silent wrong answer in place of an actionable one. The tool now returns a tool-level error naming `repograph init`. The interactive repair the CLI offers is unavailable to a server, so the message names the `--no-prompt` flag form.

**R4 — `repograph_find` is lexical-only.** The MCP surface passes `semantic = false` unconditionally. Semantic retrieval is a build-time feature that is absent from every distributed binary and, when present, adds model-loading latency to a call an agent makes mid-conversation. Predictable latency matters more here than recall, and the CLI keeps `--semantic` for the deliberate case.

**R5 — The workspace MSRV rises from 1.85 to 1.88.** `rmcp` requires 1.88. Not flagged in the proposal because it was not known until the dependency was resolved. It is user-visible for `cargo install` on an older toolchain, so `rust-version` was bumped explicitly rather than left to fail at compile time.

**R7 — the `claude mcp add` path was removed after it broke test hermeticity.** D11 originally deferred Claude Code registration to its own CLI. Implementing it exposed two faults that the "polite convention" argument had hidden. First, a vendor CLI ignores the injected `home`/`cwd` that every other host-touching function in this codebase takes for testability — so `register` wrote to `$HOME/.claude.json` while `status` probed `cwd/.mcp.json`, and the two disagreed about what was registered. That produced a spurious `warn` in the first full test run. Second, and worse, shelling out means the test suite mutates whatever real Claude config exists on the machine running it; that it happened to stay contained here was an accident of the fixture's `HOME` override, not a property of the design. Registration now writes `~/.claude.json` (user) or `.mcp.json` (project) directly, like every other agent. Merging preserves the unrelated session and project state Claude Code keeps in that file, which was the only real concern behind the original decision.

**R8 — registration scope is probed rather than stored.** `doctor` and reconciliation both need to know which scope an agent was registered at, and config records no such thing. Rather than adding a field, `status` probes user scope then project scope and takes the first registration it finds — mirroring what the artifact freshness check already does. Reconciliation re-registers at whichever scope already holds an entry, so a project-scope user does not silently acquire a second user-scope one.

**R6 — `now_rfc3339` was consolidated into `crates/repograph/src/timestamp.rs`.** It existed as byte-identical private copies in `commands/context.rs` and `commands/doctor.rs`; the MCP `context` and `doctor` tools would have made a third. Envelope parity depends on both surfaces stamping the same format, so one definition is load-bearing rather than cosmetic.

## Migration Plan

Additive; no user action required. Existing installs keep working with no MCP server until `repograph init` is re-run — `doctor` reports the server as unregistered rather than erroring, and the refreshed artifact body tells the agent to use the CLI when no tools are present.

Rollback is removing the registration (`claude mcp remove repograph`, or deleting the merged key); the binary and every CLI surface are unaffected.

## Open Questions

- Whether `repograph_context` should accept a token budget or truncation parameter. Inlined `CLAUDE.md` bodies across a large registry can be substantial, and unlike the CLI the consumer here is always a context window. Deferred to implementation once real payload sizes are measured.
- Whether `mcp serve` should expose MCP *resources* (repos as addressable URIs) in addition to tools. Plausible fit, but it widens the surface D2 deliberately caps; explicitly out of scope for this change.

**R9 — `repograph init` and `doctor --fix` register MCP, not just reconciliation.** D10 described reconciliation as the upgrade path, but a fresh `init` needs the same work done, and `doctor --fix` reports a stale registration it would otherwise refuse to repair. All three call the same `register_all`, so there is one code path and three triggers rather than three implementations.

**R10 — 18 pre-existing `collapsible_if` warnings were fixed.** `cargo clippy -- -D warnings` was already failing on `master` before this change, in files it does not touch — a newer clippy flags nested `if let` chains that `let_chains` stabilisation made collapsible. Task 9.1 is a gate on this change, so the gate was cleared mechanically via `cargo clippy --fix`. Strictly unrelated scope, applied because leaving the build red would have meant reporting the change complete against a failing check.

**R11 — registration must take an explicit scope, not infer one.** The first implementation gave `init` the scope-inferring `register_all`, which falls back to user scope when it finds no existing entry. That is right for reconciliation and `doctor --fix`, which repair an unknown setup, and wrong for `init`, which was handed a `--scope` by the user. The consequence was not cosmetic: a `--scope project` install wrote into the home directory, and because several existing acceptance tests run `init` without redirecting `HOME`, running the test suite registered repograph in the developer's own Claude and Cursor configs. Caught by inspecting the real `~/.claude.json` after a suite run. `init` now calls `register_all_at` with its own scope; `register_all` remains for the two callers that genuinely have no scope to go on. A regression test asserts a project-scope install leaves the home directory untouched, and the suite is verified to leave the host's real config byte-identical.
