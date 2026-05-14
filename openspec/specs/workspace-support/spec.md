# workspace-support Specification

## Purpose
TBD - created by archiving change workspace-support. Update Purpose after archive.
## Requirements
### Requirement: Workspace create

The CLI SHALL register a new, empty workspace identified by a user-supplied name and optionally a description, persisting the entry to the user's config file. The name MUST satisfy the workspace naming rules (see *Workspace naming rules* below). The newly created workspace SHALL have an empty `members` list.

#### Scenario: Successful create with explicit description

- **WHEN** no workspace named `acme` exists and the user runs `repograph workspace create acme --description "Acme rebuild"`
- **THEN** the config file is created/updated with a `[workspace.acme]` entry containing `description = "Acme rebuild"` and `members = []`, the exit code is `0`, and stderr confirms the creation

#### Scenario: Successful create without description

- **WHEN** no workspace named `acme` exists and the user runs `repograph workspace create acme`
- **THEN** the config file is created/updated with a `[workspace.acme]` entry with no `description` field and `members = []`, and the exit code is `0`

#### Scenario: Name conflict

- **WHEN** a workspace named `acme` is already registered and the user runs `repograph workspace create acme`
- **THEN** the exit code is `5`, stderr names the conflicting workspace, and the existing config is unchanged

#### Scenario: Invalid name rejected

- **WHEN** the user runs `repograph workspace create "Has Spaces"` (or any name violating the naming rules)
- **THEN** the exit code is `2`, stderr explains the violation and names the offending input, and no config file is written or modified

#### Scenario: Reserved name rejected

- **WHEN** the user runs `repograph workspace create default` (or `all`, or `none`)
- **THEN** the exit code is `2`, stderr explains that the name is reserved, and no config file is written or modified

### Requirement: Workspace deletion

The CLI SHALL remove a workspace identified by name and persist the change. Removal SHALL NOT affect any registered repos — `[repo.<name>]` entries are untouched.

#### Scenario: Successful workspace rm

- **WHEN** a workspace named `acme` exists and the user runs `repograph workspace rm acme`
- **THEN** the `[workspace.acme]` entry is removed from the config file, the exit code is `0`, stderr confirms, and every `[repo.<name>]` entry is unchanged

#### Scenario: Workspace rm of nonexistent name

- **WHEN** no workspace named `acme` exists and the user runs `repograph workspace rm acme`
- **THEN** the exit code is `3`, stderr explains, and the existing config is unchanged

### Requirement: Workspace listing

The CLI SHALL list all registered workspaces. Output mode SHALL follow the registry-core output contract: a `comfy-table` rendering on TTY (columns: name, description, member count) and a JSON envelope otherwise. The empty case SHALL produce a valid empty rendering, never an error. Listing order SHALL be deterministic.

#### Scenario: TTY table ls

- **WHEN** stdout is a TTY, two workspaces are registered, and the user runs `repograph workspace ls`
- **THEN** stdout contains a `comfy-table` rendering with columns for name, description, and member count; stderr is empty of diagnostics; exit code is `0`

#### Scenario: JSON ls when piped

- **WHEN** two workspaces are registered and the user runs `repograph workspace ls` with stdout redirected to a file or pipe
- **THEN** stdout contains valid JSON of shape `{ "workspaces": [ { "name", "description", "members": [...] }, ... ] }`; exit code is `0`

#### Scenario: JSON ls with explicit flag

- **WHEN** two workspaces are registered and the user runs `repograph workspace ls --json` regardless of TTY state
- **THEN** stdout contains valid JSON of the documented shape; exit code is `0`

#### Scenario: Empty workspaces JSON

- **WHEN** no workspaces are registered and the user runs `repograph workspace ls --json`
- **THEN** stdout contains exactly `{ "workspaces": [] }` (parseable JSON, empty array, never `null`); exit code is `0`

#### Scenario: Ls ordering is deterministic

- **WHEN** workspaces are created in the order `zeta`, `alpha`, `mid` and the user runs `repograph workspace ls --json`
- **THEN** the entries appear in alphabetical order by name across multiple invocations

### Requirement: Workspace show

The CLI SHALL render the details of one workspace by name, resolving each member name against the repo registry. Members whose names are no longer registered SHALL be reported as *dangling* (see *Tombstone semantics for dangling members*). Output mode SHALL follow the registry-core output contract.

#### Scenario: TTY table show

- **WHEN** stdout is a TTY, a workspace named `acme` exists with two live registered members `api` and `ui`, and the user runs `repograph workspace show acme`
- **THEN** stdout contains a `comfy-table` rendering whose rows show each member's name, path, description, and stack as resolved from `[repo.<name>]`; stderr is empty of diagnostics; exit code is `0`

#### Scenario: JSON show envelope shape

- **WHEN** a workspace named `acme` exists with live members `api` and `ui` and the user runs `repograph workspace show acme --json`
- **THEN** stdout contains valid JSON of shape `{ "name": "acme", "description": <string or null>, "members": [{"name": "api", "path": "...", "description": ..., "stack": [...]}, ...], "dangling": [] }`; exit code is `0`

#### Scenario: Show with dangling member separates live and tombstoned

- **WHEN** workspace `acme` has members `api`, `ui`, `ghost` and `ghost` has been deregistered from the repo registry, and the user runs `repograph workspace show acme --json`
- **THEN** stdout JSON contains `"members": [...]` listing only the two live, fully-resolved entries and `"dangling": ["ghost"]`; stderr contains a warning naming the dangling member; exit code is `0`

#### Scenario: Show with dangling member on TTY warns

- **WHEN** workspace `acme` has a dangling member `ghost`, stdout is a TTY, and the user runs `repograph workspace show acme`
- **THEN** stdout contains the table of live members only, stderr contains a warning explicitly naming `ghost` as a dangling reference, exit code is `0`

#### Scenario: Show of nonexistent workspace

- **WHEN** no workspace named `acme` exists and the user runs `repograph workspace show acme`
- **THEN** the exit code is `3`, stderr explains, and no JSON is written to stdout

### Requirement: Workspace member add

The CLI SHALL attach one or more registered repos to an existing workspace, persisting the change. Adding a repo already in the workspace SHALL be a no-op for that repo (no error). When multiple repos are supplied, the operation SHALL be atomic: if any named repo is not registered or the workspace is missing, no changes SHALL be written to the config file. Members SHALL be sorted alphabetically on write to ensure round-trip stability.

#### Scenario: Add a single repo to a workspace

- **WHEN** workspace `acme` exists with `members = []`, repo `api` is registered, and the user runs `repograph workspace add acme api`
- **THEN** the workspace's `members` becomes `["api"]`, the exit code is `0`, stderr confirms

#### Scenario: Add multiple repos in one call

- **WHEN** workspace `acme` exists with `members = []`, repos `api`, `ui`, and `libs` are all registered, and the user runs `repograph workspace add acme api ui libs`
- **THEN** the workspace's `members` becomes `["api", "libs", "ui"]` (alphabetically sorted), the exit code is `0`

#### Scenario: Add already-member is idempotent

- **WHEN** workspace `acme` has `members = ["api"]`, repo `api` is registered, and the user runs `repograph workspace add acme api`
- **THEN** the workspace's `members` is still `["api"]`, the exit code is `0`

#### Scenario: Add nonexistent workspace

- **WHEN** no workspace named `acme` exists and the user runs `repograph workspace add acme api`
- **THEN** the exit code is `3`, stderr names the missing workspace, and the config is unchanged

#### Scenario: Add nonexistent repo is atomic

- **WHEN** workspace `acme` exists with `members = []`, repos `api` and `ui` are registered, repo `ghost` is NOT registered, and the user runs `repograph workspace add acme api ghost ui`
- **THEN** the exit code is `3`, stderr names `ghost` as the missing repo, the workspace's `members` is still `[]` (no partial application), and no other config state is changed

### Requirement: Workspace member remove

The CLI SHALL detach one or more repos from an existing workspace, persisting the change. Removing a repo that is not currently a member SHALL be a no-op for that repo (no error). Removing a repo from a workspace SHALL NOT deregister the repo from the repo registry.

#### Scenario: Remove a single member

- **WHEN** workspace `acme` has `members = ["api", "ui"]` and the user runs `repograph workspace remove acme api`
- **THEN** the workspace's `members` becomes `["ui"]`, the exit code is `0`, the `[repo.api]` entry is unchanged

#### Scenario: Remove multiple members in one call

- **WHEN** workspace `acme` has `members = ["api", "libs", "ui"]` and the user runs `repograph workspace remove acme api libs`
- **THEN** the workspace's `members` becomes `["ui"]`, the exit code is `0`

#### Scenario: Remove non-member is idempotent

- **WHEN** workspace `acme` has `members = ["api"]` and the user runs `repograph workspace remove acme ui`
- **THEN** the workspace's `members` is still `["api"]`, the exit code is `0`

#### Scenario: Remove from nonexistent workspace

- **WHEN** no workspace named `acme` exists and the user runs `repograph workspace remove acme api`
- **THEN** the exit code is `3`, stderr explains, and the config is unchanged

### Requirement: List filtered by workspace

The CLI SHALL extend the existing `repograph list` command with an optional `--workspace <name>` filter that restricts the rendered output to repos that are live members of the named workspace. The flag SHALL NOT affect existing `list` behavior when omitted. Dangling members of the named workspace SHALL be silently skipped from the filtered list output (since `list` describes live repos); dangling surfaces remain on `workspace show` and Phase 5 `doctor`.

#### Scenario: List filtered to a workspace TTY

- **WHEN** repos `api`, `ui`, and `libs` are registered, workspace `acme` has `members = ["api", "ui"]`, stdout is a TTY, and the user runs `repograph list --workspace acme`
- **THEN** stdout contains a `comfy-table` rendering with exactly the rows for `api` and `ui`; exit code is `0`

#### Scenario: List filtered to a workspace JSON

- **WHEN** repos `api`, `ui`, and `libs` are registered, workspace `acme` has `members = ["api", "ui"]`, and the user runs `repograph list --workspace acme --json`
- **THEN** stdout contains valid JSON of shape `{ "repos": [ { "name": "api", ... }, { "name": "ui", ... } ] }`; exit code is `0`

#### Scenario: List filtered by workspace skips dangling members

- **WHEN** repo `api` is registered, repo `ghost` is NOT registered, workspace `acme` has `members = ["api", "ghost"]`, and the user runs `repograph list --workspace acme --json`
- **THEN** stdout JSON contains only `api` in `repos`, no entry for `ghost`, exit code is `0`, and stderr contains no dangling warning from `list`

#### Scenario: List filtered by nonexistent workspace

- **WHEN** no workspace named `acme` exists and the user runs `repograph list --workspace acme`
- **THEN** the exit code is `3`, stderr explains, and no list output is written to stdout

#### Scenario: List without --workspace unchanged

- **WHEN** workspaces exist and the user runs `repograph list` without `--workspace`
- **THEN** the output matches the registry-core `list` contract exactly (all registered repos, alphabetically sorted)

### Requirement: Tombstone semantics for dangling members

The CLI SHALL leave repo names intact in workspace `members` arrays when their corresponding `[repo.<name>]` entries are deregistered. The `registry-core` `remove` command SHALL NOT be modified — workspaces remain unaware to that command. Workspace read paths (`show` in particular) SHALL detect dangling members at read time, separate them from live members in JSON output, emit a stderr warning naming the dangling members on TTY, and otherwise succeed (exit `0`). Re-registering a repo with the same name SHALL restore the dangling member to live status without any further action.

#### Scenario: Repo remove leaves workspace member intact

- **WHEN** repo `api` is registered, workspace `acme` has `members = ["api"]`, and the user runs `repograph remove api`
- **THEN** the `[repo.api]` entry is removed, the `[workspace.acme] members` is still `["api"]`, and the exit code is `0`

#### Scenario: Dangling member re-registers cleanly

- **WHEN** workspace `acme` has `members = ["api"]` with `api` dangling, and the user re-runs `repograph add <path>/api --name api`
- **THEN** `repograph workspace show acme --json` next emits `members` containing the resolved `api` entry and `dangling = []`, with no further user action

#### Scenario: Registry-core remove behavior unchanged

- **WHEN** repo `api` is registered (in zero, one, or many workspaces) and the user runs `repograph remove api`
- **THEN** exit code, stderr message, and JSON behavior of `remove` are identical to the registry-core archived contract — no new fields, no new warnings, no change in behavior on the registry-core surface

### Requirement: Workspace naming rules

The CLI SHALL enforce a strict naming policy for workspaces at write time. A valid workspace name MUST match the regular expression `^[a-z0-9][a-z0-9-]{0,62}$` — lowercase ASCII letters, digits, and hyphens; must start with a letter or digit; maximum length 63 characters. The names `default`, `all`, and `none` are reserved and MUST be rejected. Existing `[repo.<name>]` entries SHALL NOT be subjected to these rules — the registry-core spec is invariant.

#### Scenario: Lowercase alphanumeric and hyphens accepted

- **WHEN** the user runs `repograph workspace create acme-rebuild-2026`
- **THEN** the workspace is created and the exit code is `0`

#### Scenario: Uppercase rejected

- **WHEN** the user runs `repograph workspace create AcmeRebuild`
- **THEN** the exit code is `2`, stderr explains the lowercase rule, and the config is unchanged

#### Scenario: Leading hyphen rejected

- **WHEN** the user runs `repograph workspace create -acme`
- **THEN** the exit code is `2`, stderr explains the start-character rule, and the config is unchanged

#### Scenario: Overlength rejected

- **WHEN** the user runs `repograph workspace create <a-64-character-name>`
- **THEN** the exit code is `2`, stderr explains the length cap, and the config is unchanged

#### Scenario: Empty name rejected

- **WHEN** the user runs `repograph workspace create ""`
- **THEN** the exit code is `2`, stderr explains the rule, and the config is unchanged

#### Scenario: Reserved names rejected

- **WHEN** the user runs `repograph workspace create default` (or `all`, or `none`)
- **THEN** the exit code is `2`, stderr explicitly names the reservation, and the config is unchanged

### Requirement: Workspace persistence and TOML schema

The CLI SHALL store workspaces alongside repos in the same `<config-dir>/config.toml` using `[workspace.<name>]` table entries with fields `description` (optional string) and `members` (array of strings, sorted alphabetically on write). The `members` array MAY be empty. Writes SHALL remain atomic (temp-file + rename) as established by registry-core. Round-trip stability MUST hold: loading and re-saving without further mutation MUST produce byte-identical content. Unknown fields on `[workspace.<name>]` entries SHALL be tolerated on load to preserve forward compatibility.

#### Scenario: Workspace and repo entries coexist

- **WHEN** the user runs `repograph add <tempdir>/api --name api` and then `repograph workspace create acme --description "Acme rebuild"` and then `repograph workspace add acme api`
- **THEN** the config file contains both `[repo.api]` and `[workspace.acme]` entries, each with their documented fields, and `[workspace.acme].members` equals `["api"]`

#### Scenario: Members sorted on write

- **WHEN** the user runs `repograph workspace add acme ui api libs` (against an existing `acme` workspace and registered repos)
- **THEN** the persisted `[workspace.acme].members` is `["api", "libs", "ui"]` in alphabetical order

#### Scenario: Empty members allowed

- **WHEN** the user runs `repograph workspace create acme`
- **THEN** the persisted `[workspace.acme]` entry has `members = []` and the next load round-trips identically

#### Scenario: Round-trip stability

- **WHEN** a config file containing both `[repo.<name>]` and `[workspace.<name>]` entries is loaded, mutated, written, loaded again, and written again with no further changes
- **THEN** the second-written content is byte-identical to the first-written content

#### Scenario: Unknown workspace fields are tolerated

- **WHEN** the config file contains a `[workspace.acme]` entry with an unknown field (e.g. a future-version field) alongside `members = []`
- **THEN** the load succeeds, the workspace is usable, and the load does not fail

#### Scenario: First workspace write creates the directory and file

- **WHEN** no config file exists and the user runs `repograph workspace create acme`
- **THEN** the config directory and file are created, and the file contains a single `[workspace.acme]` entry with no `[repo.*]` entries

### Requirement: Output contract for workspace commands

The CLI SHALL apply the registry-core output contract to every workspace subcommand and to `list --workspace`: pure data on stdout (`comfy-table` on TTY, JSON envelope when piped or `--json`), all diagnostics, confirmations, progress, and dangling warnings on stderr. Workspace commands SHALL NOT mix the two streams.

#### Scenario: Workspace JSON pipes cleanly to jq

- **WHEN** two workspaces are registered and the user runs `repograph workspace ls --json | jq '.workspaces | length'`
- **THEN** `jq` receives valid JSON without diagnostic text contaminating stdout, and outputs the count of registered workspaces

#### Scenario: Dangling warning never reaches stdout

- **WHEN** workspace `acme` has a dangling member `ghost` and the user runs `repograph workspace show acme --json > /tmp/out 2> /tmp/err`
- **THEN** `/tmp/out` parses as valid JSON of the documented shape with `dangling = ["ghost"]`, and `/tmp/err` contains the warning naming `ghost`

#### Scenario: Workspace create confirmation on stderr only

- **WHEN** the user runs `repograph workspace create acme > /tmp/out 2> /tmp/err`
- **THEN** `/tmp/out` is empty and `/tmp/err` contains the success confirmation

### Requirement: Exit codes for workspace commands

The CLI SHALL exit with the codes defined in CLAUDE.md for every workspace subcommand and for `list --workspace`. No new exit codes are introduced.

| Code | Workspace scenario |
|------|--------------------|
| `0`  | Any successful workspace operation |
| `2`  | Invalid workspace name; bad CLI arguments to a workspace subcommand |
| `3`  | `workspace rm`, `show`, `add`, `remove`, or `list --workspace` against a missing workspace; `workspace add` referencing a missing repo |
| `5`  | `workspace create` against an existing name |

#### Scenario: Bad CLI arguments produce usage error

- **WHEN** the user runs `repograph workspace add` with no `<workspace>` or `<repo>` argument
- **THEN** clap emits a usage message on stderr and the exit code is `2`

#### Scenario: Missing-workspace exit code is uniform

- **WHEN** no workspace named `acme` exists and the user runs each of `repograph workspace rm acme`, `repograph workspace show acme`, `repograph workspace add acme any`, `repograph workspace remove acme any`, and `repograph list --workspace acme`
- **THEN** every invocation exits with code `3`

#### Scenario: Name conflict on create

- **WHEN** workspace `acme` already exists and the user runs `repograph workspace create acme`
- **THEN** the exit code is `5`

