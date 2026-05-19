# git-status Specification

## Purpose
TBD - created by archiving change git-status. Update Purpose after archive.
## Requirements
### Requirement: Status reports per-repo working-tree and branch state

The CLI SHALL produce a `RepoStatus` for each in-scope registered repository, comprising: registered `name`, canonical `path`, current `branch` (or `None` for detached/unborn/bare/missing repos), `upstream` tracking branch (or `None`), `ahead` and `behind` commit counts as `u32`, a `dirty` boolean, `staged`/`unstaged`/`untracked` counts as `u32`, a coarse `state` enum drawn from `{ clean, dirty, detached, unborn, bare, missing }`, and an `error` string that is `None` for healthy repos and `Some(msg)` for repos in `missing`/`bare` state or whose `--fetch` step failed. All values SHALL be derived via `git2` exclusively. No fallback to a shelled-out `git` is permitted.

#### Scenario: Clean repo on a tracked branch reports clean

- **WHEN** a registered repo at `<tempdir>/r` is on branch `main` tracking `origin/main`, has a clean working tree, and is up to date with its upstream
- **THEN** its status row has `branch = "main"`, `upstream = "origin/main"`, `ahead = 0`, `behind = 0`, `dirty = false`, `staged = 0`, `unstaged = 0`, `untracked = 0`, `state = "clean"`, `error = null`

#### Scenario: Dirty working tree reports dirty with discrete counters

- **WHEN** a registered repo has one staged change, two unstaged modifications, and three untracked files
- **THEN** its status row has `staged = 1`, `unstaged = 2`, `untracked = 3`, `dirty = true`, `state = "dirty"`, `error = null`

#### Scenario: Detached HEAD is reported as detached

- **WHEN** a registered repo has `HEAD` detached at a commit (no current branch)
- **THEN** its status row has `branch = null`, `upstream = null`, `state = "detached"`, `error = null`; stderr emits a `warn!` line naming the repo and the short SHA

#### Scenario: Repo with no commits is reported as unborn

- **WHEN** a registered repo has been `git init`-ed but has zero commits (no `HEAD` reference yet)
- **THEN** its status row has `branch = null`, `upstream = null`, `ahead = 0`, `behind = 0`, `staged`/`unstaged`/`untracked` reflect any working-tree files, `state = "unborn"`, `error = null`

#### Scenario: Bare repository is reported as bare

- **WHEN** a registered repo was created with `git init --bare` (or otherwise reports `Repository::is_bare()`)
- **THEN** its status row has `state = "bare"`, `error = "bare repository"`, and the working-tree counters are zero

#### Scenario: Registered path no longer exists

- **WHEN** a repo registered as `foo` at `<tempdir>/foo` has had its directory removed from disk
- **THEN** its status row has `state = "missing"`, `error = "<filesystem error message>"`, and the working-tree/branch/upstream fields are their zero values

#### Scenario: Registered path is no longer a git repo

- **WHEN** a repo registered as `foo` at `<tempdir>/foo` still exists as a directory but the `.git` directory has been removed
- **THEN** its status row has `state = "missing"`, `error` is a message naming the path and indicating it is no longer a git repository

#### Scenario: Branch has no upstream

- **WHEN** a registered repo is on a local-only branch that has never been pushed and has no `branch.<name>.remote` configuration
- **THEN** its status row has `branch = "<local-branch>"`, `upstream = null`, `ahead = 0`, `behind = 0`, `state` reflects working-tree cleanliness (`clean` or `dirty`)

#### Scenario: Ahead of upstream

- **WHEN** a registered repo is on `main` tracking `origin/main`, has two local commits not in the upstream, and a clean working tree
- **THEN** its status row has `ahead = 2`, `behind = 0`, `dirty = false`, `state = "clean"`

#### Scenario: Behind upstream

- **WHEN** a registered repo is on `main` tracking `origin/main`, the upstream ref has three commits the local branch lacks, and the working tree is clean
- **THEN** its status row has `ahead = 0`, `behind = 3`, `dirty = false`, `state = "clean"`

#### Scenario: Ahead and behind simultaneously

- **WHEN** a registered repo has both diverged from its upstream — one local commit and two upstream commits
- **THEN** its status row has `ahead = 1`, `behind = 2`

### Requirement: Status scope is positional names XOR workspace, default is all

The CLI SHALL accept zero or more positional repo names and an optional `--workspace <name>` flag. Positional names and `--workspace` SHALL be mutually exclusive; specifying both SHALL exit `2` with a clap-style usage message on stderr. When neither is provided, the scope SHALL be all registered repositories in alphabetical order by name. Duplicate positional names SHALL be silently deduplicated before scanning.

#### Scenario: No arguments scans all registered repos

- **WHEN** three repos `alpha`, `mid`, `zeta` are registered and the user runs `repograph status --json`
- **THEN** stdout JSON contains exactly three entries in alphabetical order by name, exit code is `0`

#### Scenario: Positional names select a subset

- **WHEN** `alpha`, `mid`, `zeta` are registered and the user runs `repograph status alpha zeta --json`
- **THEN** stdout JSON contains exactly two entries (`alpha`, `zeta`) in alphabetical order, exit code is `0`

#### Scenario: Duplicate positional names are deduplicated

- **WHEN** `foo` is registered and the user runs `repograph status foo foo --json`
- **THEN** stdout JSON contains exactly one entry for `foo`, exit code is `0`

#### Scenario: Unknown positional name fails fast

- **WHEN** `foo` is registered and the user runs `repograph status foo ghost`
- **THEN** the exit code is `3`, stderr names `ghost` as not found, and no status table or JSON is written to stdout

#### Scenario: Names plus --workspace is a usage error

- **WHEN** the user runs `repograph status foo --workspace acme`
- **THEN** the exit code is `2` and stderr explains that names and `--workspace` are mutually exclusive

#### Scenario: --workspace filter resolves live members

- **WHEN** a workspace `acme` has live members `api` and `ui` plus one dangling member `ghost`, and the user runs `repograph status --workspace acme --json`
- **THEN** stdout JSON contains exactly the rows for `api` and `ui` (the dangling member is silently skipped), exit code is `0`

#### Scenario: Unknown workspace fails fast

- **WHEN** no workspace named `acme` exists and the user runs `repograph status --workspace acme`
- **THEN** the exit code is `3` and stderr names `acme` as not found

### Requirement: Status output contract — TTY table vs JSON envelope

The CLI SHALL render to stdout in one of two modes determined by stdout TTY detection and the `--json` flag, matching the `registry-core` output contract. TTY mode SHALL produce a `comfy-table` rendering with columns `name`, `branch`, `upstream`, `ahead`, `behind`, `dirty`, `state`. JSON mode SHALL produce the envelope `{ "repos": [ <RepoStatus>, ... ] }` with stable field order. The `error` field SHALL ALWAYS be present per row, serialized as `null` when there is no error (never omitted). All diagnostics, warnings, and progress SHALL emit to stderr only.

#### Scenario: TTY table renders the documented columns

- **WHEN** stdout is a TTY, three repos are registered, and the user runs `repograph status`
- **THEN** stdout contains a `comfy-table` rendering with the columns `name`, `branch`, `upstream`, `ahead`, `behind`, `dirty`, `state`; stderr is empty of error-level diagnostics; exit code is `0`

#### Scenario: JSON envelope when piped

- **WHEN** stdout is redirected to a pipe and the user runs `repograph status`
- **THEN** stdout contains valid JSON matching `{ "repos": [ { "name": ..., "path": ..., "branch": ..., "upstream": ..., "ahead": <u32>, "behind": <u32>, "dirty": <bool>, "staged": <u32>, "unstaged": <u32>, "untracked": <u32>, "state": "<enum>", "error": null|"..." }, ... ] }`; exit code is `0`

#### Scenario: --json forces JSON regardless of TTY

- **WHEN** stdout is a TTY and the user runs `repograph status --json`
- **THEN** stdout contains the JSON envelope; no table is rendered

#### Scenario: Empty scope produces an empty envelope

- **WHEN** no repos are registered and the user runs `repograph status --json`
- **THEN** stdout contains exactly `{ "repos": [] }`; exit code is `0`

#### Scenario: Error field is present and null for healthy repos

- **WHEN** a registered repo is in any non-error state (`clean`, `dirty`, `detached`, `unborn`) and the user runs `repograph status --json`
- **THEN** its row in stdout JSON includes the literal field `"error": null` (never absent)

#### Scenario: JSON pipes cleanly to jq

- **WHEN** the user runs `repograph status --json | jq '[.repos[] | select(.dirty)]'`
- **THEN** jq receives valid JSON without diagnostic contamination on stdout and produces the filtered subset

#### Scenario: Diagnostics never reach stdout

- **WHEN** a registered repo is missing on disk, stdout is a TTY, and the user runs `repograph status`
- **THEN** stdout contains only the table; the per-repo warning lands on stderr

### Requirement: Per-repo failures do not abort batch invocations

The CLI SHALL surface per-repo failures (missing path, broken git directory, fetch error) as a populated `error` field on that repo's `RepoStatus` plus a stderr `warn!` line, without aborting the batch. Batch exit code SHALL be `0` when the scope was "all repos" or a `--workspace` filter, regardless of how many rows have populated `error` fields. When the scope was a single positional name that resolves to a broken repo, the exit code SHALL be `3` to match `registry-core`'s not-found contract.

#### Scenario: Batch with one missing repo exits zero

- **WHEN** three repos `alpha`, `gone`, `zeta` are registered, the directory for `gone` has been removed, and the user runs `repograph status --json`
- **THEN** stdout JSON contains three entries — `alpha` and `zeta` healthy, `gone` with `state = "missing"` and a populated `error` field — and the exit code is `0`; stderr contains one `warn!` line naming `gone`

#### Scenario: Workspace filter with one dangling repo and one missing repo

- **WHEN** workspace `acme` has live members `api` (present on disk) and `gone` (registered but directory removed) plus one dangling member `ghost`, and the user runs `repograph status --workspace acme --json`
- **THEN** stdout JSON contains two rows — `api` healthy, `gone` with `state = "missing"` and a populated `error` — the dangling member `ghost` is silently skipped, and the exit code is `0`

#### Scenario: Single explicit name that points at a missing repo exits 3

- **WHEN** repo `gone` is registered but its directory has been removed, and the user runs `repograph status gone`
- **THEN** the exit code is `3` and stderr names `gone` as broken/missing

#### Scenario: Permission denied on a registered path

- **WHEN** a registered repo's directory exists but is not readable by the current user, and the user runs `repograph status --json`
- **THEN** the offending row has `state = "missing"` and `error` describing the permission failure; the batch exit code is `0`

### Requirement: --fetch is opt-in, bounded, and per-repo isolated

The CLI SHALL NOT touch the network by default. When `--fetch` is passed, the CLI SHALL `git2::Remote::fetch` only the upstream of each repo's current branch before computing `ahead`/`behind`. Repos in `detached`, `unborn`, `bare`, or `missing` state SHALL skip the fetch step. A fetch failure on any given repo SHALL populate that repo's `error` field with the fetch error message and SHALL NOT prevent the rest of the batch from completing.

#### Scenario: Default invocation does not touch the network

- **WHEN** the user runs `repograph status --json` without `--fetch`
- **THEN** no remote network call is made for any repo (no `git2::Remote::fetch` is invoked)

#### Scenario: --fetch updates ahead/behind for a tracked branch

- **WHEN** a registered repo is on `main` tracking `origin/main`, the local branch is two commits behind a remote that has new commits since the last manual fetch, and the user runs `repograph status --fetch --json`
- **THEN** the row's `behind` field reflects the post-fetch count

#### Scenario: --fetch failure on one repo is isolated

- **WHEN** three repos are registered, the upstream remote of one fails to fetch (e.g. credential failure, network error), and the user runs `repograph status --fetch --json`
- **THEN** the failing repo's row has a populated `error` field, the other two rows reflect their fetched state, and the exit code is `0`

#### Scenario: --fetch skips repos without a fetchable upstream

- **WHEN** a registered repo is in `detached` / `unborn` / `bare` / `missing` state and the user runs `repograph status --fetch`
- **THEN** no fetch is attempted for that repo; its `error` field reflects the underlying state (or is `null` for `detached`/`unborn`)

### Requirement: Parallel scan with progress, cleared before stdout

The CLI SHALL scan repositories concurrently using `rayon`'s default global thread pool. When stdout is a TTY, the CLI SHALL render an `indicatif::MultiProgress` on stderr with one spinner per repo and SHALL drop the `MultiProgress` (clearing all spinners) before writing the first byte of the final rendering to stdout. When stdout is not a TTY (`--json` or piped), no spinners SHALL be drawn.

#### Scenario: TTY mode draws spinners and clears before rendering

- **WHEN** five repos are registered, stdout is a TTY, and the user runs `repograph status`
- **THEN** during the scan stderr displays a spinner per repo; once scanning completes the spinners are cleared before the table is written to stdout

#### Scenario: Non-TTY mode draws no spinners

- **WHEN** five repos are registered, stdout is redirected to a pipe, and the user runs `repograph status`
- **THEN** stderr contains no spinner output; only structured `tracing` log lines (and any per-repo warnings) appear there

#### Scenario: Spinner output never contaminates stdout

- **WHEN** the user runs `repograph status > out.json --json` against any registered repos
- **THEN** the bytes written to `out.json` are valid JSON with no spinner escape sequences

### Requirement: Exit codes follow the documented contract

The CLI SHALL exit with the codes defined in `CLAUDE.md`: `0` success, `1` general failure, `2` usage error, `3` resource not found, `4` permission denied, `5` conflict. The `status` command SHALL NOT introduce new exit codes.

#### Scenario: Successful batch exits 0 even with partial errors

- **WHEN** `repograph status` runs against three repos and at least one has a populated `error` field but the scope was not a single explicit name
- **THEN** the exit code is `0`

#### Scenario: Unknown positional name exits 3

- **WHEN** the user runs `repograph status ghost` and no repo named `ghost` is registered
- **THEN** the exit code is `3`

#### Scenario: Names plus --workspace exits 2

- **WHEN** the user runs `repograph status foo --workspace acme`
- **THEN** the exit code is `2`

#### Scenario: Malformed TOML on config load exits 1

- **WHEN** the user runs `repograph status` and the config file exists but is syntactically invalid TOML
- **THEN** the exit code is `1` and stderr names the parse error

