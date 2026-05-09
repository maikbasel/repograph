# registry-core

Persistent registry of local git repositories. Owns the `Config`/`Repo` model, the TOML schema at `<config-dir>/config.toml`, the JSON envelope output contract, and the exit-code map. Every later capability composes against this one.

## Purpose

Make `repograph` capable of registering, listing, and removing local git repositories with persistent TOML-backed state — the foundation for workspaces (Phase 3), git introspection (Phase 4), agent context (Phase 5), and the future MCP server.

## Requirements


### Requirement: Add registers a local git repository

The CLI SHALL register a local git repository identified by an absolute, canonicalized path with either a user-supplied or path-derived name, persisting the entry to the user's config file. The path MUST be a valid git repository as determined by `git2::Repository::open`.

#### Scenario: Successful add with explicit name

- **WHEN** a real git repository exists at `<tempdir>/myrepo` and the user runs `repograph add <tempdir>/myrepo --name foo`
- **THEN** the config file is created/updated with a `[repo.foo]` entry, the exit code is `0`, and stderr confirms the registration

#### Scenario: Add infers name from path basename

- **WHEN** a real git repository exists at `<tempdir>/myrepo` and the user runs `repograph add <tempdir>/myrepo` without `--name`
- **THEN** the repository is registered under the name `myrepo`, the exit code is `0`

#### Scenario: Path stored as canonical absolute

- **WHEN** the user runs `repograph add ./relative/path --name foo` from a working directory that contains a git repo at that relative location
- **THEN** the stored `path` field in TOML is the canonical absolute path (symlinks resolved), not the relative spelling

#### Scenario: Description and stack metadata

- **WHEN** the user runs `repograph add <tempdir>/myrepo --name foo --description "hello" --stack rust,cli`
- **THEN** the stored entry has `description = "hello"` and `stack = ["rust", "cli"]`

#### Scenario: Path is not a git repository

- **WHEN** the user runs `repograph add <tempdir>/plain-dir` against a path that exists but is not a git repository
- **THEN** the exit code is `3`, stderr explains the failure, and no config file is written or modified

#### Scenario: Path does not exist

- **WHEN** the user runs `repograph add /nonexistent/path`
- **THEN** the exit code is `3`, stderr explains the failure, and no config file is written or modified

#### Scenario: Name conflict

- **WHEN** a repository named `foo` is already registered and the user runs `repograph add <other-tempdir>/repo2 --name foo`
- **THEN** the exit code is `5`, stderr names the conflicting entry, and the existing config is unchanged

#### Scenario: Path conflict

- **WHEN** a repository at `<tempdir>/myrepo` is already registered as `foo` and the user runs `repograph add <tempdir>/myrepo --name bar`
- **THEN** the exit code is `5`, stderr names the conflicting entry by name and path, and the existing config is unchanged

### Requirement: List renders the registered repositories

The CLI SHALL list all registered repositories. Output mode SHALL be determined by stdout TTY detection and the presence of `--json`: a `comfy-table` rendering on TTY, a JSON envelope otherwise. The empty-registry case SHALL produce a valid empty rendering, never an error.

#### Scenario: TTY table list

- **WHEN** stdout is a TTY, two repositories are registered, and the user runs `repograph list`
- **THEN** stdout contains a `comfy-table` rendering with columns for name, path, description, and stack; stderr is empty of diagnostics; exit code is `0`

#### Scenario: JSON list when piped

- **WHEN** two repositories are registered and the user runs `repograph list` with stdout redirected to a file or pipe
- **THEN** stdout contains valid JSON of shape `{ "repos": [ { "name": ..., "path": ..., "description": ..., "stack": [...] }, ... ] }`; exit code is `0`

#### Scenario: JSON list with explicit flag

- **WHEN** two repositories are registered and the user runs `repograph list --json` regardless of TTY state
- **THEN** stdout contains valid JSON of the documented shape; exit code is `0`

#### Scenario: Empty registry JSON

- **WHEN** no repositories are registered and the user runs `repograph list --json`
- **THEN** stdout contains exactly `{ "repos": [] }` (parseable JSON, empty array, never `null`); exit code is `0`

#### Scenario: Empty registry table

- **WHEN** no repositories are registered, stdout is a TTY, and the user runs `repograph list`
- **THEN** stdout contains either a header-only table or a short "no repositories registered" line on stderr with empty stdout (implementation choice documented in design); exit code is `0`

#### Scenario: List ordering is deterministic

- **WHEN** repositories are registered in the order `zeta`, `alpha`, `mid` and the user runs `repograph list --json`
- **THEN** the entries appear in alphabetical order by name across multiple invocations

### Requirement: Remove deregisters a repository by name

The CLI SHALL remove a registered repository identified by name and persist the change to the config file.

#### Scenario: Successful remove

- **WHEN** a repository named `foo` is registered and the user runs `repograph remove foo`
- **THEN** the entry is removed from the config file, the exit code is `0`, stderr confirms the removal

#### Scenario: Remove of nonexistent name

- **WHEN** no repository named `foo` is registered and the user runs `repograph remove foo`
- **THEN** the exit code is `3`, stderr explains, and the existing config is unchanged

### Requirement: Output contract — stdout is data, stderr is diagnostics

The CLI SHALL emit pure data to stdout (a `comfy-table` rendering on TTY, a JSON envelope when piped or `--json` is set) and SHALL emit all diagnostics, progress, confirmations, and warnings to stderr. No command output mixes the two streams.

#### Scenario: JSON pipes cleanly to jq

- **WHEN** the user runs `repograph list --json | jq '.repos | length'`
- **THEN** `jq` receives valid JSON without diagnostic text contaminating stdout, and outputs the count of registered repositories

#### Scenario: Diagnostics never reach stdout

- **WHEN** `repograph add <tempdir>/myrepo --name foo` succeeds
- **THEN** stdout is empty (or contains only the documented add-output, if any) and the success confirmation appears on stderr

### Requirement: Exit codes follow the documented contract

The CLI SHALL exit with the codes defined in CLAUDE.md: `0` success, `1` general failure, `2` usage error, `3` resource not found, `4` permission denied, `5` conflict.

#### Scenario: Bad CLI arguments produce usage error

- **WHEN** the user runs `repograph add` with no `<path>` argument
- **THEN** clap emits a usage message on stderr and the exit code is `2`

#### Scenario: Permission denied on config write

- **WHEN** the configured config directory exists but is not writable by the current user, and the user runs `repograph add <tempdir>/myrepo --name foo`
- **THEN** the exit code is `4` and stderr explains the permission failure

#### Scenario: Malformed TOML on load

- **WHEN** the config file exists but contains syntactically invalid TOML, and the user runs any subcommand that loads config
- **THEN** the exit code is `1` and stderr names the file and the parse error

### Requirement: Config persistence

The system SHALL resolve its config directory using the following precedence (highest first): the `--config-dir <PATH>` global CLI flag, the `REPOGRAPH_CONFIG_DIR` environment variable, the platform default `dirs::config_dir() / "repograph"`. The system SHALL store the registry as TOML at `<config-dir>/config.toml`, using `[repo.<name>]` table entries. The system SHALL create the config directory as needed on first write. Writes SHALL be atomic (temp-file + rename) so a crash mid-write cannot corrupt the file.

#### Scenario: First write creates the directory and file

- **WHEN** no config file exists and the user runs `repograph add <tempdir>/myrepo --name foo`
- **THEN** the config directory and file are created, and the file contains a single `[repo.foo]` entry

#### Scenario: Round-trip stability

- **WHEN** a config file is loaded, mutated, written, loaded again, and written again with no further changes
- **THEN** the second-written content is byte-identical to the first-written content

#### Scenario: Unknown fields are tolerated

- **WHEN** the config file contains a `[repo.foo]` entry with an unknown field (e.g. a future-version field)
- **THEN** the load succeeds, the unknown field is preserved on the next save (or, if preservation is impractical, dropped — implementation choice documented in design), and the load does not fail

#### Scenario: Empty registry produces no spurious file

- **WHEN** no `repograph` command has ever written, and the user runs `repograph list`
- **THEN** the config file is not created (a missing file means an empty registry)

#### Scenario: --config-dir flag overrides env var

- **WHEN** `REPOGRAPH_CONFIG_DIR` is set to `<dir-A>`, the user runs `repograph --config-dir <dir-B> add <tempdir>/myrepo --name foo`, and `<dir-A>` ≠ `<dir-B>`
- **THEN** the config file is written under `<dir-B>` and `<dir-A>` is unchanged

#### Scenario: Env var honored when flag is absent

- **WHEN** `REPOGRAPH_CONFIG_DIR` is set to `<dir-A>`, no `--config-dir` flag is passed, and the user runs `repograph add <tempdir>/myrepo --name foo`
- **THEN** the config file is written under `<dir-A>`

#### Scenario: --config-dir is global across subcommands

- **WHEN** the user runs each of `repograph --config-dir <dir> add …`, `repograph --config-dir <dir> list`, and `repograph --config-dir <dir> remove …`
- **THEN** every subcommand reads from and writes to `<dir>` (the flag is accepted on all subcommands without per-subcommand declaration)

#### Scenario: Platform has no default config dir and no override

- **WHEN** `dirs::config_dir()` returns `None`, `REPOGRAPH_CONFIG_DIR` is unset, no `--config-dir` flag is passed, and the user runs any subcommand that requires config
- **THEN** the exit code is `1` and stderr instructs the user to pass `--config-dir`
