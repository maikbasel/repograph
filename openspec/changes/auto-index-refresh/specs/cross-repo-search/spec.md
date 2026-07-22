## ADDED Requirements

### Requirement: Auto-refresh the index on find

`repograph find` SHALL refresh the search index for the repos it is about to query before running the query, unless `--no-refresh` is given. Refresh SHALL be gated by a working-tree staleness check: for each in-scope repo the system SHALL compare the newest modification time across that repo's git-tracked files against a per-repo baseline recorded at the last index; a repo SHALL be refreshed only when its working tree is newer than the baseline, when it has no baseline, or when no index exists. The staleness check SHALL NOT read or re-hash file contents (it inspects modification times only). Refreshing a repo SHALL reuse the incremental, git-aware indexing path, so only changed files are reprocessed. Refresh SHALL detect uncommitted working-tree edits, not only committed changes. All refresh diagnostics SHALL be written to stderr; the stdout data contract for `find` SHALL be unchanged. When `--no-refresh` is given, `find` SHALL query the existing index without refreshing.

#### Scenario: Uncommitted edit is reflected without a manual index

- **WHEN** a tracked file in a registered repo is modified but not committed, and the user runs `repograph find "<query>"`
- **THEN** that repo is reindexed incrementally before the query runs
- **AND** results reflect the edited working-tree content
- **AND** refresh diagnostics are written to stderr only

#### Scenario: Unchanged repos are not reindexed

- **WHEN** the user runs `repograph find "<query>"` twice with no file changes between runs
- **THEN** the second run reprocesses no files
- **AND** the staleness check reads no file contents for the unchanged repos

#### Scenario: Refresh is scoped to the queried repos

- **WHEN** the user runs `repograph find "<query>" --workspace acme`
- **THEN** only repos belonging to workspace `acme` are considered for refresh
- **AND** repos outside the workspace are left untouched in the index

#### Scenario: Refresh can be disabled

- **WHEN** the user runs `repograph find "<query>" --no-refresh`
- **THEN** the index is not refreshed
- **AND** the query runs against the existing index contents

### Requirement: Index records a per-repo working-tree baseline

When indexing a repo (via `repograph index` or an auto-refresh), the system SHALL record, per repo, the newest modification time across that repo's git-tracked files, persisted alongside the existing indexed-commit record. This baseline SHALL be the value the auto-refresh staleness check compares against. An index predating this baseline (no recorded value) SHALL be treated as stale on the next `find`, causing exactly one refresh that then records the baseline.

#### Scenario: Baseline is written on index

- **WHEN** the user runs `repograph index`
- **THEN** each indexed repo has a working-tree modification-time baseline persisted with its indexed-commit record

#### Scenario: Pre-existing index without a baseline refreshes once

- **WHEN** `repograph find` runs against an index built before baselines were recorded
- **THEN** each in-scope repo is refreshed once
- **AND** its baseline is recorded so a subsequent unchanged `find` reprocesses no files

### Requirement: Indexing reports per-repo progress

While indexing more than one repo, the system SHALL report progress that identifies the repo currently being processed (name and position within the scope) on stderr, so a long-running index is distinguishable from a hang. Progress output SHALL go to stderr only and SHALL be cleared before the command returns, preserving the stdout data contract.

#### Scenario: Progress advances per repo

- **WHEN** the user runs `repograph index` over several registered repos in a terminal
- **THEN** the progress indicator updates to name each repo as it is processed
- **AND** the indicator is cleared before the final summary is written

## MODIFIED Requirements

### Requirement: Corrupt or unreadable index is surfaced

The system SHALL detect a corrupt or unreadable index database and surface it as an error on stderr with exit code 1, rather than panicking or returning partial results silently. A missing index (never built) SHALL NOT be treated as corruption. With auto-refresh enabled (the default), `repograph find` SHALL build the missing index, then run the query, exiting 0. Only when `--no-refresh` is given SHALL `repograph find` treat a missing index as an error, reporting that no index exists and directing the user to run `repograph index`, exiting with code 3.

#### Scenario: Find before any index exists auto-builds

- **WHEN** the user runs `repograph find "<query>"` before ever running `repograph index`
- **THEN** the index is built for the in-scope repos
- **AND** the query runs against the freshly built index
- **AND** the command exits with code 0

#### Scenario: Find with --no-refresh before any index exists

- **WHEN** the user runs `repograph find "<query>" --no-refresh` before ever running `repograph index`
- **THEN** stderr explains that no index exists and to run `repograph index`
- **AND** the command exits with code 3

#### Scenario: Corrupt index database

- **WHEN** the index database is present but unreadable or corrupt
- **THEN** stderr reports the failure
- **AND** the command exits with code 1
