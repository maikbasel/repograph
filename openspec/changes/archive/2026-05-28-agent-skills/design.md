## Context

repograph currently learns the user's agent toolchain via `[agents].selected` in config, but the link is one-way: repograph knows about Claude Code / Cursor / Aider / Windsurf / agents-md / Copilot, but those agents don't know about repograph. The user has to hand-author the prose that tells their agent "when the user asks about cross-repo context, run `repograph context --json`."

The previously-planned `repograph-mcp` binary would have closed this loop via the Model Context Protocol — a second binary that exposes `repograph context`, `repograph list`, `repograph status`, `repograph switch`, and `repograph doctor` as MCP tools. That requires (1) maintaining a second binary, (2) a second distribution channel, (3) an MCP runtime, and (4) clients that speak MCP. Today, all six supported agents instead read a native instruction file from a well-known path. Writing those instruction files at `repograph init` time is a strictly simpler path to the same outcome and works with every agent we already support — no new protocol, no new binary.

The constraints that shape the design:

- **No new dependencies.** `fs-err`, `serde`, `dirs`, `tracing`, `cliclack` are already in the workspace. The implementation is plain string concatenation + file writes.
- **Idempotent re-runs.** Users will run `repograph init` more than once (settings-panel flow, agent selection updates). Each run must converge on the same artifact contents, never duplicate, and never clobber user-authored content adjacent to the managed section.
- **Per-agent native format.** Each agent's path and frontmatter convention is fixed by the agent itself; we cannot ship a single common file format.
- **Scope semantics differ per agent.** `agents-md` (`AGENTS.md`) and `aider` (`CONVENTIONS.md`) are project-root conventions only — there is no user-scope path for them. Other agents support both.
- **Output contract preserved.** `init` already emits all UI to stderr; artifact-install diagnostics MUST follow the same rule.
- **Existing init flow is large and tested.** The change is purely additive at the flow level: a new step after agent selection. No existing step changes.

## Goals / Non-Goals

**Goals:**

- One installer surface (`repograph-core::agent_artifact`) that owns the shared body, per-agent format wrappers, the delimiter contract, and the idempotency rules. Testable independently of the CLI flow.
- `repograph init` writes a native artifact per selected agent in both the interactive and `--no-prompt` paths.
- Re-running `init` against existing artifacts produces no diffs unless the body actually changed (template version bump) or `--force` was passed.
- Files the user may already author (`AGENTS.md`, `CONVENTIONS.md`) are managed by a delimiter pair so the install never destroys surrounding content.
- A single migration note in README + clear `--help` output explains the new flag, the per-agent matrix, and the `repograph-mcp` deprecation.

**Non-Goals:**

- No MCP binary, no protocol layer, no daemon. Explicitly canceled.
- No file-watcher. Re-running `init` is the path to refresh artifacts; we don't watch the config for changes.
- No automatic artifact removal when an agent is removed from `[agents].selected`. Documented as manual cleanup.
- No Copilot artifact in v1. Copilot's instruction format varies across surfaces (repo-level vs editor-level vs Copilot Workspace) and we don't have a single converged path yet. Deferred to a follow-up change.
- No per-user templating of the artifact body. The skill content invokes `repograph` itself, which reads the user's config at run time — there's no static state to bake in.
- No artifact uninstall command. If a user wants the file gone, `rm` is the path; not worth a subcommand.

## Decisions

### D1: Split into a new core capability `agent-skills` rather than extending `init-command`

The artifact-writing machinery (shared body, per-agent format wrappers, delimiter contract, idempotency, the `ArtifactResult` type) is a coherent unit that is testable in isolation and has no dependency on the init flow. It also has a plausible second consumer: a future `repograph install-skills` command (if we ever decide users want to re-install without rerunning the full init flow) or a `repograph doctor --fix` mode that re-writes missing artifacts.

**Alternative considered:** Fold everything into `commands/init.rs` as private helpers. Rejected because: (a) the artifact body grows over time and naturally wants to live in `repograph-core` next to the agent registry (`agents.rs`); (b) testing the delimiter contract against many edge cases is cleaner as a core-level unit test than wired through the CLI; (c) the presentation/logic split in CLAUDE.md mandates that "no terminal output, no `println!`" code lives in core, which is exactly the artifact-writing surface.

### D2: Single shared body as `const &str` in `agent_artifact.rs`

The "when to invoke repograph" prose, the CLI command table, the JSON schema cross-reference — all of this is identical for every agent. We write it once as `pub const BODY: &str = ...` and let each per-agent writer wrap it in native frontmatter/headers.

**Alternative considered:** Per-agent body authored separately. Rejected: bug-prone (the CLI surface changes → six files diverge), and 80% of the prose is identical. The 20% that differs is structural (frontmatter / heading levels), which the wrapper layer owns.

**Alternative considered:** Template the body with handlebars/tera. Rejected: no per-user values to substitute. The skill calls `repograph` which reads the user's config; baking the config snapshot into the artifact would make re-runs needed on every registry change. Static body is correct.

### D3: Managed-section delimiter pair for files with potential user content

For artifacts at paths the user may already maintain — `AGENTS.md`, `CONVENTIONS.md` — the writer wraps repograph-managed content in:

```
<!-- repograph:begin -->
<repograph-managed body>
<!-- repograph:end -->
```

Install algorithm:

1. Read existing file (if any).
2. If delimited block exists and contents == new content: no-op.
3. If delimited block exists and contents differ: rewrite only the delimited region; preserve everything outside.
4. If no delimited block: append a leading newline (if file non-empty) + the delimited block to the end.
5. `--force` skips steps 1–4 and writes the file fresh.

Files at exclusive-repograph paths (`.claude/skills/repograph/SKILL.md`, `.cursor/rules/repograph.mdc`, `.windsurfrules`) don't strictly need the delimiter — repograph owns the whole file. We still emit it for consistency (a future "remove repograph block from this file" tool can use the same delimiters everywhere).

**Alternative considered:** File checksum stored separately (`.repograph-manifest`). Rejected: extra state to maintain, doesn't help with files that share content (`AGENTS.md` could have user content above/below the repograph block), and the delimiter approach is self-contained.

**Alternative considered:** Always overwrite without checking. Rejected: clobbers `AGENTS.md` / `CONVENTIONS.md` that users wrote themselves before adding repograph.

### D4: `--scope` defaults to `user`

The most common case is a developer running repograph for their own toolchain on their own machine. User-scope artifacts cover everything once. Project-scope is for teams that want repograph-aware instructions to ship with the repo (committed to git). Defaulting to `user` matches "I want this to work for me" semantics; opting into project requires intent.

**Alternative considered:** No default, require explicit `--scope`. Rejected: hostile to interactive use; the prompt asks the same question more cheaply.

**Alternative considered:** Detect from context — if the working directory is inside a registered repo, use project; otherwise user. Rejected: hard to reason about, surprising when the same command produces different artifacts depending on `pwd`.

### D5: Project-only agents (`agents-md`, `aider`) silently ignore `--scope user`

These two agents have no user-scope path by convention. AGENTS.md is a project-root standard; CONVENTIONS.md is read by aider only relative to the current working directory. We could (a) error out, (b) silently fall through to project, (c) skip the artifact entirely.

We choose **(b) silently fall through to project**, with a `tracing::info!` log line stating "agents-md is project-scope only; writing to project root regardless of --scope user". Rationale: the user picked `agents-md`; they want the artifact; the only path is project. Erroring is hostile; skipping silently breaks the contract that selecting an agent writes its artifact.

### D6: `--no-prompt` requires `--scope` only when at least one selected agent has a meaningful scope choice

`--no-prompt --agents agents-md` does not need `--scope` (only one possible target). `--no-prompt --agents claude-code` does (user vs project both valid). The validation checks the selected agent list before deciding whether `--scope` is mandatory.

**Alternative considered:** Always require `--scope` under `--no-prompt`. Rejected: forces ceremony in cases where the choice doesn't exist. The error message would have to say "but you can pick anything, it doesn't matter" — which is the same as not requiring it.

### D7: Drop the `repograph-mcp` plan

The proposal is explicit about removing forward-pointing references to `repograph-mcp` from `README.md` and from doc comments / design.md files that mention it as planned. Archive files (the contents of `openspec/changes/archive/<change>/`) stay frozen by convention — we update the *current* references, not the historical record.

### D8: Skill content advertises read-only commands; mutating commands are not included in the v1 trigger list

The artifact tells the agent when to invoke `repograph context --json`, `repograph list --json`, `repograph status --json`, `repograph switch <name>`, `repograph doctor --json`. It does *not* tell the agent to invoke `repograph add`, `repograph remove`, `repograph workspace ...`, `repograph init`. Rationale: registry management is the user's responsibility; auto-mutating it from an agent is a footgun. If a user wants the agent to add a repo, the prompt path is "ask the user to run `repograph add` themselves."

### D9: Resolved deviations during implementation

**Scope type naming and module placement.** The new `Scope` enum lives at `repograph_core::agent_artifact::Scope` and is NOT re-exported at the crate root. `lib.rs` already re-exports `context::Scope` at the root (a different concept — context-aggregation scope, not artifact-install scope). Two `Scope` types under the same root would force callers to disambiguate with full module paths anyway, and renaming the new one to `ArtifactScope` would force three spec/design rewrites. Leaving the new `Scope` at module scope keeps the spec/design text intact and forces explicit `agent_artifact::Scope` use at every call site, which doubles as documentation. The existing `context::Scope` re-export stays unchanged.

**Clap parsing happens in the binary, not core.** Task 1.2 originally called for `clap::ValueEnum` on the core `Scope`. That would require pulling `clap` into `repograph-core`, which CLAUDE.md forbids ("`repograph-core` — domain library (no clap, no terminal output)"). Resolved: define `Scope` in core with `serde::Serialize` only; in `init.rs` (binary), use `#[arg(long, value_parser = parse_scope)]` with a small `parse_scope: fn(&str) -> Result<Scope, String>` helper that maps `"user"`/`"project"` to the core enum. Same user-facing CLI surface; no layering violation.

**`install_one` splits whole-file owners from shared-file agents.** The original `install_one` ran every agent through `splice_managed_section`, including claude-code and cursor whose `render_artifact` output includes YAML frontmatter. The splice contract preserves bytes *outside* the delimiters and rewrites only the delimited region — so for a fresh write it emitted just `<begin>\n<body>\n<end>\n`, dropping the YAML frontmatter for whole-file owners. Unit tests caught nothing because they only exercised `AgentsMd` (frontmatter-less). The acceptance suite surfaced it. Resolved: added `wholly_owned_file(agent) -> bool` returning `true` for `ClaudeCode` and `Cursor`; `install_one` now writes `render_artifact(agent)` verbatim for those agents (byte-comparing for `Unchanged`) and runs the splice path only for the shared-file agents (`AgentsMd`, `Aider`, `Windsurf`).

**`UsageError` maps to exit code 2.** The CLAUDE.md contract documents exit 2 as "usage error (bad arguments)", but `RepographError::UsageError` was previously mapped to 1. The `agent-skills` spec (`init-command` MODIFIED requirement) calls for exit 2 on the `--no-prompt` / missing `--scope` validation, which surfaces a `UsageError`. Resolved: updated `RepographError::exit_code()` so `UsageError` → 2 alongside `InvalidName` and `NeedsInit`. Pre-existing callers (`add::derive_name`, `main::resolve_config_dir`) are genuine usage errors and now correctly exit 2; this fixes a latent inconsistency with the contract.

## Risks / Trade-offs

- **[Risk] User-edited content inside the delimited block gets clobbered on re-run.** → Mitigation: docs explicitly state the delimited region is repograph-managed; `--force` and "remove the delimiters to keep my edits" are documented escape hatches. The delimiter format is a comment so it survives most renderers.
- **[Risk] A future agent adds a new instruction-file convention and we ship without it.** → Mitigation: the `agent-skills` capability is the single point to extend; adding a new agent is a per-agent writer plus a test fixture. Documented in the capability spec.
- **[Risk] Body drift between the artifact and the actual CLI surface.** → Mitigation: a test reads the artifact body and asserts that every command name mentioned is a real `clap` subcommand (parsed via the existing `<Cli as CommandFactory>::command()` helper from the `completions` work).
- **[Risk] AGENTS.md / CONVENTIONS.md merge conflicts in team repos when two team members both run init.** → Mitigation: the delimited block is identical across runs, so conflicts are zero unless the body version changed; document the file as a managed artifact in the team's contributing guide; the file's contents are deterministic so re-running `init` resolves the conflict by overwriting the block.
- **[Trade-off] Static body means the agent has to call `repograph list` itself to discover the registry.** → Acceptable: that's the design; the agent uses the CLI rather than reading a snapshot. The trade is freshness vs. simplicity, and freshness wins.
- **[Risk] On Windows, `.windsurfrules` may need CRLF line endings.** → Mitigation: write with `\n` and let downstream tools handle it; document in README that `.windsurfrules` is read by Windsurf which is line-ending-tolerant. If a user reports breakage, address it in a follow-up.

## Open Questions

- **Should the install step prompt for confirmation when overwriting an existing artifact whose delimited content differs?** Leaning toward no — re-runs are expected to produce identical content; if the user has a specific concern, `--force` and `git diff` are the right tools. This stays no-prompt unless someone surfaces a UX problem during implementation.
- **Should `repograph doctor` get a check that verifies installed artifacts are still present and current?** Out of scope here; tracked as a follow-up if useful. The doctor capability is already shipped and well-scoped; adding an artifact-freshness check is a natural extension but not part of this change.
