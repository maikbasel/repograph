## Context

Phase 1 (`registry-core`, archived) gave us a flat registry: `[repo.<name>]` entries keyed by name in a single `~/.config/repograph/config.toml`, atomically written, with a hardened output contract (stdout = data, stderr = diagnostics) and a documented exit-code map (0/1/2/3/4/5). Every later capability composes against this foundation.

Workspaces are Phase 2. They are a layer above the registry — a workspace is a named, ordered set of references into the existing repo namespace. The motivation is bluntly practical: a flat list of every registered repo on disk is unusable for both a thirty-repo developer and a context-window-constrained agent. Phase 3 (`status`) and Phase 4 (`context`) become tractable once we can scope queries to "the three repos that make up project X".

Constraints driving this design:

- **The `registry-core` spec is archived and must stay invariant.** Any design that requires changing `[repo.<name>]` semantics or the behavior of `repograph remove` is off the table. Workspaces compose against the registry, never against it.
- **The output contract is a hard guarantee.** stdout is data (JSON envelope or `comfy-table`), stderr is everything else. Agent consumers parse stdout; humans skim stderr. Mixing the two is a contract violation.
- **The TOML config is human-editable.** Round-trip stability (`save → load → save` byte-identical) is a registry-core invariant we extend, not relax.
- **One config file, not two.** Workspaces live alongside repos in the same `config.toml`. `repograph doctor` (Phase 5) needs cross-cutting visibility; splitting files would force every read path to open both.

Industry prior art (surveyed at proposal time) converges strongly enough that several decisions write themselves; this document focuses on the calls that needed thought, plus the calls that need to be visible to future contributors (and to the archive auditor) when they read this back.

## Goals / Non-Goals

**Goals:**

- Ship six workspace subcommands (`create`, `rm`, `ls`, `show`, `add`, `remove`) end-to-end with TTY and JSON output, real `git2`-backed acceptance tests, and the registry-core output contract intact.
- Ship `repograph list --workspace <name>` as the smallest filter that proves workspaces pay off on Day 1 of Phase 2 — without leaning on `status` or `context`, which belong to later phases.
- Define `[workspace.<name>]` TOML schema, including round-trip stability and forward-compatibility (unknown fields tolerated).
- Establish *tombstone semantics* for dangling members: deregistering a repo never silently mutates workspaces; dangling references surface at read time (stderr warning + JSON `dangling` field), and full cleanup is deferred to Phase 5 `doctor`.
- Enforce a strict naming policy on workspaces at write time (RFC 1123 label style + reserved names), so future filter flags, shell completion, and JSON keys never trip over user-supplied identifiers.

**Non-Goals:**

- `workspace rename` — defer until requested. Renaming would also need to consider what happens to in-flight references; we don't have a real use case yet.
- `workspace use` / persistent active-workspace state — flag-only filtering is sufficient and keeps agent-facing output stateless.
- Multi-workspace filter on `list` (e.g. `--workspace a --workspace b`) — single workspace covers the demonstrated need; defer composition.
- Glob/pattern membership — explicit names only. Globbing introduces ambiguity that pairs badly with tombstones.
- `repograph doctor` — Phase 5 owns it. Phase 2 only surfaces dangling state; it does not offer cleanup.
- Effects on `status` and `context` — those are Phases 3 and 4. The `--workspace` flag pattern they will adopt is established here, but they don't ship in this change.
- Modifying `registry-core` behaviors. `add` / `list` / `remove` keep their archived contract. Even `list` only gains a new optional argument; its existing behavior when the argument is omitted is unchanged.

## Decisions

### Decision 1 — Membership lives on the workspace (group owns members)

**Choice:** `[workspace.<name>] members = ["repo1", "repo2"]`. The workspace table holds an array of bare repo names. `[repo.<name>]` entries do not learn about workspaces.

**Why:** Industry-dominant pattern (Cargo, npm, pnpm, VS Code `.code-workspace` all converge). Keeps the registry pure — repos are identified by path and metadata; group membership is a separate concern. Diffing the workspace's TOML table tells you the whole story of that workspace; you don't need to grep `[repo.*]` to find members. And — critically — repo deregistration doesn't have to walk the workspace table, which is the linchpin that keeps the `registry-core` `remove` command invariant.

**Alternatives considered:**

- *Repos store workspace membership* (`[repo.api] workspaces = ["acme"]`). Symmetrically possible but forces `registry-core` to learn about workspaces — out of bounds — and makes "what's in workspace X?" an O(N) scan instead of an O(1) lookup.
- *Denormalize both ways.* Two sources of truth that can drift. Rejected.

### Decision 2 — Member references use bare repo names, not paths

**Choice:** `members = ["api", "ui"]` (names). Resolution against `[repo.<name>]` happens at read time.

**Why:** Path is owned by the registry. Storing paths in a workspace would create a second source of truth for "where is `api` on disk?" — a registry rename or relocation would then have to update every workspace. Names are stable identifiers (the user picked them); paths can move under the user's feet (filesystem reorgs, mount changes). Names also make tombstone semantics trivial: a dangling member is just a name with no matching `[repo.<name>]` entry; re-registering with the same name restores it for free.

**Alternatives considered:**

- *Store paths inline.* Would let a workspace point at unregistered directories, but at the cost of duplicating the registry's job and losing the cleanness of "workspaces are views over the registry". Punted.
- *Store an opaque ID per repo.* Overkill for a single-user CLI; introduces stable-identifier complexity for no payoff.

### Decision 3 — Tombstone semantics for dangling references

**Choice:** When a repo is deregistered (`repograph remove <repo>`), its name is left intact in any workspace `members` arrays. `repograph remove` is **not modified** — it remains unaware of workspaces. Read paths (`workspace show`, future `doctor`) detect dangling members and surface them: stderr warning on TTY, dedicated `dangling: [...]` field in `workspace show`'s JSON envelope. `repograph list --workspace` silently skips dangling members (it describes live repos).

**Why:** Three reasons, in order of importance:

1. **Keeps `registry-core` invariant.** Cascading delete would force `remove` to walk the workspace table, modify it, and report the cascaded edits — that's a behavior change to an archived command. Tombstones cost zero churn on the registry side.
2. **Re-registration is the common case.** Users rename directories, move a repo to a new disk, recreate a repo with the same name after a fresh clone. Cascading would punish that workflow; the user re-adds, and the workspace heals automatically.
3. **`doctor` already in the roadmap.** We're not inventing a sink for dangling refs — Phase 5 needs `doctor` for unrelated reasons (paths-that-no-longer-exist, broken `git2::Repository::open`). Dangling workspace members are one more class of drift `doctor` handles.

The risk — silent drift accumulating without the user noticing — is mitigated by the stderr warning on every `workspace show` and `workspace ls` read path, plus the machine-readable `dangling` JSON field. The user (or their agent) always sees dangling state on the next read.

**Alternatives considered:**

- *Cascade silently.* Footgun: "where did my workspace member go?" Rejected.
- *Cascade with warning.* Better than silent, but still mutates state in response to an unrelated command, and forces `registry-core` `remove` to know about workspaces.
- *Block remove with conflict exit code.* Punishes the common case (developer wants to re-add by the same name).
- *Hard-fail like Cargo when a member is missing.* Too strict for a multi-repo CLI — rename one repo, every workspace breaks. Rejected.

### Decision 4 — Strict naming policy on workspaces (RFC 1123 label)

**Choice:** Workspace names match `^[a-z0-9][a-z0-9-]{0,62}$` and exclude reserved words `default`, `all`, `none`. Enforced at write time on `workspace create`, exit code `2` on violation. Existing `[repo.<name>]` entries are **not** subject to these rules.

**Why:** Kubernetes' RFC 1123 label rule is the most-tested precedent for short, identifier-ish names that survive being a JSON key, a shell completion target, a CLI argument, a URL path segment (in case repograph ever grows a remote API), and a TOML key. Reserving `default`/`all`/`none` anticipates future filter ergonomics — we don't want `--workspace all` to be ambiguous when someone names a workspace `all`. Enforcing at write time fails fast: invalid names never get persisted, never round-trip, never surprise downstream code.

Why workspaces only: the `registry-core` spec didn't define repo-name rules, and applying them retroactively would (a) churn an archived spec, (b) potentially invalidate existing user data. Workspaces are new surface — we can be strict from day one without retroactive cost.

**Alternatives considered:**

- *Permissive at write time, document gotchas.* Git remote names and AWS profile names do this and pay for it (issues filed against both about spaces breaking commands). Rejected — agents that consume `repograph` output need stable names.
- *Apply the rules to both repos and workspaces.* Touches archived spec. Rejected.

### Decision 5 — CLI surface mirrors `docker context`

**Choice:** `repograph workspace create | rm | ls | show | add | remove`. The verbs split cleanly: `create`/`rm` manage the workspace, `add`/`remove` manage membership. No `use` verb — no persistent "active workspace".

**Why:** `docker context create/ls/rm/use/inspect` is the closest peer (workspaces aren't running services, but they ARE named groupings managed via a sub-noun verb). Skipping `use` is deliberate — see Decision 7.

`add`/`remove` for membership (vs. `attach`/`detach` or `set`) follows `git remote add`. The name overlap with the top-level `repograph add` (which registers repos) is unambiguous in context: `repograph add` is a top-level verb that takes a path; `repograph workspace add` is sub-nouned and takes a workspace name plus repo names. Clap's hierarchical help disambiguates further.

`rm` is preferred over `delete` to match `docker context rm`, `git remote remove`, and the existing top-level `repograph remove`. Three letters; no ambiguity.

**Alternatives considered:**

- *`create`/`delete` for workspaces, `add`/`remove` for members.* The asymmetry (`delete` workspace, `remove` member) added cognitive load without value.
- *Single verb per side (e.g. `add` for both workspaces and members).* Conflates the layers; `workspace add foo` becomes ambiguous between "create workspace foo" and "add member foo to current".

### Decision 6 — Atomic multi-repo add, idempotent everywhere

**Choice:** `workspace add <ws> <r1> <r2> <r3>` is atomic — if any named repo is missing from the registry, no member is added, exit `3`. `workspace add` of an already-member is a no-op (no error). `workspace remove` of a non-member is a no-op (no error).

**Why:** Atomicity on add prevents a partially-applied state where the user thinks they ran a single command but the workspace ends up in an in-between configuration. The exit-3 already tells them what went wrong; making them undo three out of five inserts adds nothing.

Idempotency on add/remove follows from the set-semantics view: `members` is a set, and add/remove are set operations. Set operations are idempotent; users who script repograph (and agents that drive it) benefit from being able to re-run safely.

**Alternatives considered:**

- *Best-effort add (insert what you can, exit non-zero with a report).* Surfaces partial state through stderr but requires the consumer to parse the report to know what actually happened. Atomic + exit 3 is unambiguous.
- *Strict on duplicates (error if already a member).* Forces the caller to introspect before mutating, which a multi-repo `workspace add a b c` would have to do per-repo — annoying without payoff.

### Decision 7 — Flag-only filter on `list`, no persistent active workspace

**Choice:** `repograph list --workspace <name>`. No `repograph workspace use <name>` verb. The flag is per-command and stateless; the JSON output never depends on hidden global state.

**Why:** Cargo, npm, and pnpm all chose flag-only because there's no notion of an "active project" — the user runs the tool from wherever, and the flag's effect is local to that invocation. `repograph` fits the same shape: it's a developer/agent tool, not an infrastructure context manager. The kubectl/docker pattern (persistent + flag) introduces "wrong context" footguns — entire blog posts exist about `kubens`/`kubectx` precisely because users forget which context they're in.

For agent consumers, this matters even more: parsing the JSON output's meaning must not depend on hidden state. A future agent that asks "show me the repos in workspace X" composes naturally with `--workspace X`; if there were a hidden active workspace, the agent would have to call `workspace current` first or risk being silently wrong.

Adding `use` later is non-breaking. Not adding it now is the cheap, safe call.

**Alternatives considered:**

- *Persistent `use` verb.* See above — footgun, stateful, defers the failure to runtime.
- *Filter on `add` (`repograph add ./repo --workspace acme`).* Tempting shortcut, but conflates registry-write with workspace-membership-write. Each mutation should have one observable effect.

### Decision 8 — `dangling` field on `workspace show` JSON

**Choice:** The JSON envelope for `workspace show <name>` always includes a `dangling: [<name>, ...]` array, even when empty. Live, fully-resolved members go in `members`. Dangling member names are flagged on stderr with a warning per name.

**Why:** Agent consumers should never have to call a second command to detect drift. The `dangling` field makes inconsistency a structural feature of the response, not a side effect of running `doctor`. The field is always present (never `null`, never absent) so agents can write `len(resp.dangling) > 0` without a key-existence check — same discipline registry-core applied to its `repos: []` envelope.

The TTY rendering shows live members in a table; the warning on stderr names the dangling entries. The user gets the same information through both surfaces; the contracts agree.

**Alternatives considered:**

- *Mix live and dangling in one array with a `status` field on each entry.* More compact but harder to consume — every agent has to filter the array. Splitting them keeps the common case ("give me the live members") a direct field access.
- *Hide dangling entirely; surface only via `doctor`.* Defers a Phase-2 capability into a Phase-5 command. The JSON contract should be self-describing from Day 1.

## Risks / Trade-offs

- **[Tombstones could accumulate silently if the user never runs `workspace show`]** → Mitigation: `workspace ls` includes the member count, which is computed from the raw `members` array (dangling included). A workspace whose count differs from its live-resolved size on `show` will surface drift. Phase 5 `doctor` will sweep them up; Phase 2 only needs to make them visible.
- **[Atomic multi-repo add is a small performance hit on large batches]** → No real cost — the check is a O(N) lookup into the `BTreeMap` of repos; even 100 repos is microseconds. Mention it only because someone reading this design would ask.
- **[Naming rules diverge between repos and workspaces]** → Documented in CLAUDE.md / README. The friction is bounded — users only ever see this when `workspace create` rejects a name. The README's command surface table will note the rule.
- **[`add` is overloaded across `repograph add <path>` and `repograph workspace add <ws> <repo>`]** → Clap's hierarchical help renders each command's args clearly; documentation in README mirrors the structure. The overload is consistent with Unix conventions (`git remote add`, `docker context create` and `docker container create` share `create`).
- **[`workspace remove <repo>` looks like a deregistration]** → Mitigation: the help text and README example explicitly state that `workspace remove` only detaches from membership and does not affect the registry. Tests assert that `[repo.<name>]` is unchanged after `workspace remove`.
- **[Round-trip stability across mixed `[repo.*]` + `[workspace.*]` entries]** → Validated by a dedicated round-trip test in `crates/repograph-core` that asserts byte-equality after `load → save → load → save`. Members are sorted on write; workspace keys, like repo keys, are stored in a `BTreeMap` so the TOML library writes them in alphabetical order.
- **[Future `doctor --fix` semantics could conflict with what `workspace show` reports today]** → Open until Phase 5. The current contract — `dangling` lists names, no further information — gives `doctor` the freedom to define cleanup semantics without rewriting Phase 2 behavior. Worst case: Phase 5 adds a new field to `workspace show`, which is additive (the registry-core spec already requires forward-compatible JSON envelopes).
- **[`list --workspace` skipping dangling silently could surprise users who expect parity with `workspace show`]** → Documented in the spec scenarios. Reasoning: `list` is about *live repos* — the dangling surface lives on the *workspace* read path. A user who wants to audit dangling state runs `workspace show`. The two commands answer different questions.

## Migration Plan

There is no on-disk migration. The TOML schema extension is purely additive: existing configs with only `[repo.<name>]` entries continue to load unchanged. The first workspace command (`workspace create` or `workspace add`) is what adds the first `[workspace.<name>]` table.

No version field, no migration script. The forward-compatibility clause (unknown fields tolerated on load) covers the reverse direction too — older builds reading a config written by a newer build with unknown workspace fields will not error.

For users running the prior `repograph` build: zero action required.

## Open Questions

None at proposal time. All six exploration questions were resolved during `/opsx:explore` with industry-prior-art evidence; this design captures the resolutions and the rejected alternatives so the audit trail is preserved.

Questions that may resurface during implementation, captured here so they aren't lost:

- The exact stderr wording for the dangling warning ("workspace `acme` references unregistered repo `ghost`") needs final phrasing; tasks will pin it.
- Whether `workspace show` for an empty workspace renders a header-only table or a "no members" stderr line — registry-core left this as an implementation choice for `list`; mirror whatever pattern was chosen there for consistency.
- Whether the `description` field on a workspace should be rendered in `workspace show`'s TTY table header (as a caption) or as a separate stderr line. Cosmetic, deferred to implementation.
