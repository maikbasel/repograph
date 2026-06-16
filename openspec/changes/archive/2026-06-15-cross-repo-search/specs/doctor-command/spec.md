## ADDED Requirements

### Requirement: Doctor reports search index health

`repograph doctor` SHALL include a check reporting the state of the cross-repo search index. The check SHALL report `ok` when the index exists and every registered repo's indexed commit matches its current HEAD, `warn` when the index is missing or stale for one or more repos, and SHALL never panic when the index database is absent or unreadable. The check SHALL identify which repos are stale so the user knows to run `repograph index`.

#### Scenario: Index present and current

- **WHEN** the user runs `repograph doctor` after indexing with no subsequent commits
- **THEN** the index check reports `ok`

#### Scenario: Index missing

- **WHEN** the user runs `repograph doctor` before ever building an index
- **THEN** the index check reports `warn` indicating the index has not been built
- **AND** doctor does not panic

#### Scenario: Index stale relative to HEAD

- **WHEN** a registered repo has new commits since it was last indexed
- **THEN** the index check reports `warn` and names the stale repo
