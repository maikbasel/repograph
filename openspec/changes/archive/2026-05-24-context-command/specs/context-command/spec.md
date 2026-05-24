## ADDED Requirements

### Requirement: Context command surfaces a `repograph context` subcommand with three mutually exclusive scope modes

The CLI SHALL accept a `repograph context` subcommand that resolves an in-scope repository set from one of three modes, mutually exclusive at the argument-parsing layer:

1. **Default (no flags)** — every repository in the config registry.
2. **`--workspace <name>`** — every member of the named workspace.
3. **Positional `<repo>...`** — exactly the named repositories.

`--workspace` and positional arguments SHALL be marked mutually exclusive via clap so the command body never has to reconcile both. Invocations with no scope flag and no positional arg SHALL be treated as "default".

#### Scenario: Default scope includes every registered repo

- **WHEN** a config has three registered repos `api`, `ui`, `lib` and the user runs `repograph context`
- **THEN** the in-scope set is `["api", "lib", "ui"]` (sorted) and the resulting payload's `repos` array contains exactly those three entries

#### Scenario: Workspace scope filters to members

- **WHEN** a config has registered repos `api`, `ui`, `lib`, a workspace `backend` with members `["api", "lib"]`, and the user runs `repograph context --workspace backend`
- **THEN** the in-scope set is `["api", "lib"]` (sorted) and the payload's `repos` array contains exactly those two entries; `ui` is excluded

#### Scenario: Positional scope picks named repos

- **WHEN** a config has registered repos `api`, `ui`, `lib` and the user runs `repograph context ui api`
- **THEN** the in-scope set is `["api", "ui"]` (sorted) and the payload contains exactly those two entries; `lib` is excluded

#### Scenario: Workspace and positional are mutually exclusive

- **WHEN** the user runs `repograph context --workspace backend api`
- **THEN** clap rejects the invocation with a usage error before `run` is called; exit code is `2`; no payload is written

#### Scenario: Unknown workspace exits 3

- **WHEN** the user runs `repograph context --workspace nope` and no workspace named `nope` exists
- **THEN** the exit code is `3`, stderr names `nope`, and no payload is written to stdout

#### Scenario: Unknown positional repo exits 3

- **WHEN** the user runs `repograph context api bogus` and no repo named `bogus` is registered
- **THEN** the exit code is `3`, stderr names `bogus`, and no payload is written to stdout; the command does NOT emit a partial payload for `api`

### Requirement: Context command gates on `[agents]` via the shared auto-prompt helper

The `context` command SHALL call `ensure_agents_configured(&mut config, &config_dir)` before reading `config.agents()`. When `[agents]` is missing and stdout is a TTY, the helper SHALL render the agent multiselect, persist the user's selection, and proceed. When `[agents]` is missing and stdout is NOT a TTY, the helper SHALL return `RepographError::NeedsInit` and the command SHALL exit with code `2`.

When `[agents]` is present but `selected` is empty, the command SHALL proceed to produce a payload with empty `agent_docs` arrays per repo (configured-but-empty is a valid state, distinct from not-configured).

#### Scenario: First-use TTY invocation prompts through agent selection then produces context

- **WHEN** the config has no `[agents]` section, stdout is a TTY, the user runs `repograph context`, and completes the multiselect with two agents
- **THEN** the saved config gains `[agents] selected = [...]` with those two agents; the payload is then produced normally on stdout; exit code is `0`

#### Scenario: First-use non-TTY invocation errors with NeedsInit

- **WHEN** the config has no `[agents]` section, stdout is redirected to a pipe, and the user runs `repograph context`
- **THEN** the exit code is `2`, stderr names `repograph init`, and no payload is written to stdout; no config write occurs

#### Scenario: Empty `[agents] selected = []` produces empty agent_docs without prompting

- **WHEN** the config has `[agents] selected = []` and the user runs `repograph context --json`
- **THEN** no interactive prompt fires; the payload's `repos[*].agent_docs` arrays are empty; exit code is `0`

### Requirement: Agent file resolution walks the registry patterns against each in-scope repo root

For each in-scope repo and each agent in `config.agents().selected`, the system SHALL resolve the file patterns from the agent registry (`repograph-core::agents`) against the repo's canonical root path. Flat patterns (e.g. `CLAUDE.md`, `.cursorrules`) SHALL be resolved by direct existence check at the repo root. Glob patterns with a known parent directory (e.g. `.cursor/rules/*.md`) SHALL be resolved by listing entries of that parent directory (non-recursive) and matching against a compiled `globset::GlobSet`.

The resolver SHALL NOT walk the repo tree beyond the known parent directories defined by the registry. Files matching multiple patterns under the same agent SHALL be deduplicated by canonical path within that agent's `files` array. Files outside the repo root (e.g. via symlinks) MAY appear in the `files` array; the relative path returned SHALL be relative to the repo root.

The matched file's `path` field in the payload SHALL be the path relative to the repo root, using forward slashes. The `bytes` field SHALL be the raw byte length of the file content as read from disk. The `content` field SHALL be the file's UTF-8 contents verbatim, with no truncation.

#### Scenario: Flat patterns resolve to a single file at the repo root

- **WHEN** a registered repo contains `CLAUDE.md` at its root, the config selects `claude-code`, and the user runs `repograph context --json`
- **THEN** the payload's `repos[0].agent_docs` contains one entry with `agent = "claude-code"` and `files` containing exactly `{ path: "CLAUDE.md", bytes: <length>, content: <verbatim> }`

#### Scenario: Glob pattern expands to multiple files in a known subdirectory

- **WHEN** a registered repo contains `.cursor/rules/style.md` and `.cursor/rules/tests.md`, no `.cursorrules` file, the config selects `cursor`, and the user runs `repograph context --json`
- **THEN** the payload's `repos[0].agent_docs[?(@.agent=="cursor")].files` contains two entries with paths `".cursor/rules/style.md"` and `".cursor/rules/tests.md"` (sorted), each with correct `bytes` and verbatim `content`

#### Scenario: Mixed flat + glob patterns under one agent

- **WHEN** a registered repo contains both `.cursor/rules/style.md` and `.cursorrules`, the config selects `cursor`, and the user runs `repograph context --json`
- **THEN** the `cursor` entry's `files` array contains both files (sorted by relative path); no duplicate `.cursor/rules/style.md` entry appears even though the file matches the same agent

#### Scenario: No matching files produces an empty files array for that agent

- **WHEN** a registered repo contains none of the files for the selected agents and the user runs `repograph context --json`
- **THEN** the payload contains an entry per selected agent under `agent_docs`, each with `files: []`; the repo entry's `warnings` is `[]`; exit code is `0`

#### Scenario: File content is verbatim with no truncation

- **WHEN** a registered repo's `CLAUDE.md` is 50 KB and the user runs `repograph context --json`
- **THEN** the payload's `content` field for that file is exactly 50 KB (or equivalent length post-JSON-escaping) and matches the on-disk bytes character-for-character; no truncation marker is inserted

#### Scenario: Resolver does not walk into nested directories

- **WHEN** a registered repo has `nested/CLAUDE.md` (not at root) and no root-level `CLAUDE.md`, the config selects `claude-code`, and the user runs `repograph context --json`
- **THEN** the `claude-code` entry's `files` is `[]`; the nested copy is NOT inlined; no warning is emitted (this is expected behavior, not an error)

### Requirement: Per-file and per-repo read failures surface as inline warnings, not aborts

When reading an agent file fails (permission denied, I/O error, file deleted between stat and read) or the file is not valid UTF-8, the resolver SHALL omit the file from the `files` array and append a string to the enclosing repo's `warnings` array naming the file and the failure. When a registered repo's path no longer exists on disk, the system SHALL emit a repo entry with empty `agent_docs`, `branch: null`, and `warnings` containing a single string explaining the path is missing.

The command SHALL NOT abort on per-file or per-repo failures. Exit code SHALL remain `0` as long as the global setup (config load, agents resolution, scope resolution) succeeded.

#### Scenario: Unreadable file becomes a warning entry

- **WHEN** a registered repo has `CLAUDE.md` whose permissions prevent reading, the config selects `claude-code`, and the user runs `repograph context --json`
- **THEN** the `claude-code` entry's `files` does NOT include `CLAUDE.md`; the repo entry's `warnings` contains a single string naming `CLAUDE.md` and the permission error; exit code is `0`

#### Scenario: Non-UTF-8 file is skipped with a warning

- **WHEN** a registered repo has a `.cursorrules` file containing non-UTF-8 bytes, the config selects `cursor`, and the user runs `repograph context --json`
- **THEN** the `cursor` entry's `files` does NOT include `.cursorrules`; the repo entry's `warnings` contains a string naming `.cursorrules` and indicating the file is not valid UTF-8; exit code is `0`

#### Scenario: Missing repo path produces a placeholder entry with warning

- **WHEN** a config has a registered repo `ghost` whose path has been deleted from disk, the user runs `repograph context --json`
- **THEN** the payload's `repos` array contains an entry for `ghost` with `branch: null`, `agent_docs: []`, and `warnings` containing a single string explaining the path no longer exists; other in-scope repos still produce normal entries; exit code is `0`

#### Scenario: One repo's failures do not affect another repo's success

- **WHEN** two registered repos are in scope, repo A has a readable `CLAUDE.md` and repo B's path is missing
- **THEN** the payload's `repos` array contains both entries; repo A's entry has its `claude-code` `files` populated normally; repo B's entry has the missing-path warning; exit code is `0`

### Requirement: JSON output shape is stable and versioned

When `--json` is passed or stdout is NOT a TTY, the command SHALL emit a single JSON object to stdout with this shape:

```json
{
  "schema_version": 1,
  "generated_at": "<RFC 3339 timestamp UTC>",
  "agents": ["<agent-id>", ...],
  "scope": { "kind": "all" | "workspace" | "repos", "name"?: "<ws>", "repos"?: ["<r>", ...] },
  "repos": [
    {
      "name": "<repo-name>",
      "path": "<canonical absolute path>",
      "branch": "<branch>" | null,
      "agent_docs": [
        {
          "agent": "<agent-id>",
          "files": [ { "path": "<relpath>", "bytes": <u64>, "content": "<utf-8>" }, ... ]
        }, ...
      ],
      "warnings": ["<string>", ...]
    }, ...
  ],
  "warnings": ["<string>", ...]
}
```

`schema_version` SHALL be the integer `1` for this version of the contract. The schema SHALL be additive-only at version `1`; any breaking change SHALL bump the version. `scope.kind` SHALL be one of `"all"`, `"workspace"`, or `"repos"`. `scope.name` SHALL be present iff `kind == "workspace"`. `scope.repos` SHALL be present iff `kind == "repos"`. The top-level `repos` array SHALL be sorted by `name` ascending. Each repo's `agent_docs` array SHALL preserve the order of agents from `config.agents().selected`. Each agent's `files` array SHALL be sorted by `path` ascending.

The JSON output SHALL be a single-line emission (no trailing newline) suitable for piping into `jq` and other tools. No pretty-printing SHALL be applied unless an explicit `--pretty` flag is added in a future version.

Stdout SHALL contain only this JSON payload. All diagnostics, warnings, and progress indicators SHALL go to stderr via `tracing`.

#### Scenario: JSON payload validates as a single JSON object

- **WHEN** the user runs `repograph context --json` against any non-empty config and pipes the output through `jq '.schema_version'`
- **THEN** `jq` succeeds and prints `1`; the stdout is parseable as a single JSON object

#### Scenario: Schema includes all documented top-level fields

- **WHEN** the user runs `repograph context --json` against a config with two registered repos and the selected agents
- **THEN** the parsed JSON has all of: `schema_version`, `generated_at` (parseable as RFC 3339 UTC), `agents` (array of strings), `scope` (object), `repos` (array), and a top-level `warnings` (array)

#### Scenario: Repos are sorted by name in stable order

- **WHEN** the user runs `repograph context --json` against a config with repos registered in insertion order `zeta`, `alpha`, `mu`
- **THEN** the payload's `repos[*].name` array is `["alpha", "mu", "zeta"]`

#### Scenario: Workspace scope reflects in the scope field

- **WHEN** the user runs `repograph context --workspace backend --json`
- **THEN** the payload's `scope` is `{ "kind": "workspace", "name": "backend" }` with no `repos` field set

#### Scenario: Positional scope reflects in the scope field

- **WHEN** the user runs `repograph context api ui --json`
- **THEN** the payload's `scope` is `{ "kind": "repos", "repos": ["api", "ui"] }` (preserving user-supplied order in the scope echo) with no `name` field set

#### Scenario: Default scope reflects in the scope field

- **WHEN** the user runs `repograph context --json` with no scope flags
- **THEN** the payload's `scope` is `{ "kind": "all" }` with neither `name` nor `repos` set

#### Scenario: Non-TTY without --json emits JSON

- **WHEN** the user runs `repograph context > out.json` (stdout redirected to file)
- **THEN** `out.json` parses as the same JSON object as if `--json` had been passed explicitly; exit code is `0`

#### Scenario: Stdout contains only the payload

- **WHEN** the user runs `repograph context --json 2>/dev/null` against any valid config
- **THEN** stdout contains exactly the JSON payload (with no leading or trailing log lines, banners, or spinner artifacts)

### Requirement: TTY output renders the same data as Markdown on stdout

When stdout is a TTY and `--json` is NOT passed, the command SHALL emit the payload as a Markdown document on stdout. The document SHALL contain a top-level title naming the scope and counts, one second-level heading per repo (`## <name>  (branch: <b>)` or `(branch: detached/unborn/bare/missing)`), the repo's canonical path on the next line as inline-code, and one third-level heading per matched agent followed by a fourth-level heading per file with size, and the file's content rendered in a fenced code block.

Code-block fence selection SHALL be triple backticks (` ``` `) by default. When a matched file's content contains a triple-backtick line, the fence SHALL fall back to triple tildes (`~~~`) to avoid premature fence termination. The same data present in the JSON payload SHALL be present in the Markdown output (no truncation, no dropped fields).

Warnings (per-file, per-repo, or global) SHALL be rendered inline below the relevant heading as a blockquote prefixed with `> **warning:**`.

Stdout SHALL contain only the Markdown document. All diagnostics SHALL go to stderr via `tracing`.

#### Scenario: TTY default emits Markdown headers and code blocks

- **WHEN** stdout is a TTY, the user runs `repograph context` against a config with one registered repo `api` (branch `main`) containing `CLAUDE.md`, and `[agents] selected = ["claude-code"]`
- **THEN** stdout begins with `# repograph context` (or equivalent title naming the scope and counts), contains `## api  (branch: main)` followed by an inline-code path line and `### claude-code`, then `#### CLAUDE.md (<size>)` followed by a fenced code block containing the file's verbatim content

#### Scenario: Triple-backtick collision falls back to tilde fences

- **WHEN** a matched file's content contains a line of `` ``` ``, stdout is a TTY, and the user runs `repograph context`
- **THEN** the surrounding fence in the Markdown output for that file is `~~~` (tilde) rather than `` ``` `` (backtick); other files with no triple-backtick content still use backtick fences

#### Scenario: Warnings render as blockquotes in Markdown

- **WHEN** a repo has a missing path and the user runs `repograph context` in a TTY
- **THEN** the repo's section in the Markdown output contains a blockquote line beginning with `> **warning:**` naming the missing-path issue; the section has no `### <agent>` subheadings beyond what was actually resolved

#### Scenario: Markdown output preserves stdout-only contract

- **WHEN** the user runs `repograph context 2>err.log` in a TTY
- **THEN** stdout contains the Markdown document with no log lines interleaved; `err.log` contains the `tracing` diagnostics; the Markdown is byte-identical to running the command without stderr redirection

### Requirement: Exit codes follow the documented contract

The `context` command SHALL exit with codes defined in `CLAUDE.md`: `0` success (including success-with-warnings); `1` general failure (e.g. malformed existing TOML); `2` usage error (mutually exclusive flags, `NeedsInit` from `ensure_agents_configured` in non-TTY); `3` resource not found (named workspace not in config, named positional repo not in registry); `4` permission denied (only via `ensure_agents_configured` persisting a fresh agent selection — the read path never writes); `5` is not produced by this command (no conflict semantics).

#### Scenario: Successful invocation with warnings exits 0

- **WHEN** the user runs `repograph context` against a config where one of three repos has a missing path
- **THEN** the payload contains all three repos (one with a warning), and the exit code is `0`

#### Scenario: Unknown workspace exits 3

- **WHEN** the user runs `repograph context --workspace nope` and no `nope` workspace exists
- **THEN** the exit code is `3`

#### Scenario: Unknown positional repo exits 3

- **WHEN** the user runs `repograph context ghost` and no `ghost` repo is registered
- **THEN** the exit code is `3`

#### Scenario: Malformed TOML exits 1

- **WHEN** the user runs `repograph context` and the existing config file is not valid TOML
- **THEN** the exit code is `1` and stderr names the parse error

#### Scenario: Non-TTY without [agents] exits 2

- **WHEN** the user runs `repograph context > out.json` (non-TTY) against a config with no `[agents]` section
- **THEN** the exit code is `2`, stderr names `repograph init`, and `out.json` is empty

### Requirement: Tracing logs entry, success, and error consistently

The `context` command's `run` function SHALL emit `tracing` logs at three points:

- **Entry (`debug`)**: command name and resolved scope kind (no file contents).
- **Success (`info`)**: counts of in-scope repos, selected agents, and total payload bytes.
- **Error (`error`)**: the error itself plus relevant context (scope kind, repo or workspace name if applicable).

Per-file warnings SHALL emit at `warn` level with structured fields naming the repo and the relative file path. File contents SHALL NEVER be logged; only lengths SHALL appear in structured fields.

#### Scenario: Successful invocation emits debug entry and info success on stderr

- **WHEN** the user runs `repograph context --json` with `RUST_LOG=repograph=debug`
- **THEN** stderr contains a `DEBUG` line naming the command and scope on entry, and an `INFO` line on success with structured fields for repo count, agent count, and total bytes

#### Scenario: Warning paths emit per-warning warn lines

- **WHEN** a registered repo's `CLAUDE.md` is unreadable due to permissions
- **THEN** stderr (at default `info` level) contains a `WARN` line naming the repo, the file path, and the read failure; no file content is logged

#### Scenario: File content is never logged

- **WHEN** the user runs `repograph context` with `RUST_LOG=repograph=debug` against a repo with a 10 KB `CLAUDE.md`
- **THEN** stderr contains no portion of the file's content; structured fields name only lengths and identifiers

### Requirement: README documents the context command surface and payload shape

The project `README.md` SHALL document the `repograph context` subcommand under its command table, including:

- The three scope modes (default / `--workspace <name>` / positional `<repo>...`) with one example invocation each.
- The output contract: JSON when `--json` or non-TTY, Markdown when TTY.
- A short JSON payload example showing all top-level fields (`schema_version`, `generated_at`, `agents`, `scope`, `repos`, `warnings`).
- The exit code mapping (reuse the existing exit-code table; document that `5` is not produced by `context`).

#### Scenario: README contains a context command entry

- **WHEN** a reader opens `README.md` and searches the command table for `context`
- **THEN** they find the `repograph context` row with a one-line description, the three scope modes documented, and at least one example showing the JSON payload's top-level shape
