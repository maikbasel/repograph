# cross-repo-search Specification

## Purpose
TBD - created by archiving change cross-repo-search. Update Purpose after archive.
## Requirements
### Requirement: Build a cross-repo search index

The system SHALL provide `repograph index` to build or refresh a search index over the git-tracked files of registered repos. Indexing SHALL be scoped to all registered repos by default, or to a single workspace when `--workspace <name>` is given. Only files tracked by git SHALL be indexed; ignored and untracked files SHALL be skipped. The index SHALL be persisted to a single SQLite database at `dirs::data_dir()/repograph/index.db`, with each row attributed to its source repo.

#### Scenario: Index all registered repos

- **WHEN** the user runs `repograph index` with at least one registered repo
- **THEN** every git-tracked file in each registered repo is chunked and stored in `index.db`
- **AND** untracked and git-ignored files are excluded
- **AND** the command exits with code 0

#### Scenario: Index scoped to a workspace

- **WHEN** the user runs `repograph index --workspace acme`
- **THEN** only repos belonging to workspace `acme` are indexed
- **AND** repos outside the workspace are left untouched in the index

#### Scenario: No repos registered

- **WHEN** the user runs `repograph index` with an empty registry
- **THEN** the command reports that there is nothing to index on stderr
- **AND** exits with code 0 without creating a corrupt database

### Requirement: Incremental, git-aware reindexing

The system SHALL reindex incrementally. For each tracked file the index SHALL store a content hash; on a subsequent `repograph index` only files whose content hash differs from the stored value SHALL be re-chunked and re-embedded. Files removed from a repo since the last index SHALL be purged from the index. Re-indexing a single repo SHALL NOT require rewriting rows belonging to other repos.

#### Scenario: Unchanged repo is skipped

- **WHEN** the user runs `repograph index` twice with no file changes between runs
- **THEN** the second run re-embeds no files
- **AND** reports that the index is already up to date on stderr

#### Scenario: Only changed files are reprocessed

- **WHEN** a single tracked file is modified and `repograph index` is run again
- **THEN** only that file's chunks are replaced in the index
- **AND** chunks for unchanged files retain their previous rows

#### Scenario: Deleted files are purged

- **WHEN** a previously indexed file is deleted from a repo and `repograph index` is run again
- **THEN** that file's chunks are removed from the index
- **AND** the file no longer appears in `repograph find` results

### Requirement: Hybrid lexical and semantic retrieval

The system SHALL provide `repograph find "<query>"` returning code chunks ranked by relevance across all registered repos, or one workspace when `--workspace <name>` is given. Retrieval SHALL combine BM25 lexical matching (SQLite FTS5) with semantic vector similarity over locally computed embeddings, merging the two result sets by reciprocal-rank fusion. Semantic retrieval SHALL be opt-in; when no embedding index is present or semantic mode is disabled, the system SHALL fall back to lexical-only retrieval and report that it did so on stderr. Each result SHALL identify its repo, file path relative to the repo root, starting line, a fused relevance score, and a snippet.

#### Scenario: Fuzzy query finds a semantically similar implementation

- **WHEN** the index contains a reference implementation in one repo and the user runs `repograph find` with a natural-language description that does not share keywords with that code, with semantic retrieval enabled
- **THEN** the reference implementation appears in the ranked results
- **AND** each result lists its repo, relative path, starting line, score, and snippet

#### Scenario: Exact symbol lookup

- **WHEN** the user runs `repograph find "<exact_symbol_name>"`
- **THEN** chunks containing that symbol are returned via lexical matching
- **AND** results are ordered by fused relevance score descending

#### Scenario: Lexical fallback without a semantic index

- **WHEN** the user runs `repograph find` against an index built without embeddings
- **THEN** results are produced from lexical matching alone
- **AND** a notice that semantic retrieval was unavailable is written to stderr
- **AND** the command exits with code 0

#### Scenario: No matches

- **WHEN** a query matches nothing in the index
- **THEN** the result set is empty
- **AND** the command exits with code 0

### Requirement: Stable output contract for find

`repograph find` SHALL write pure data to stdout and all diagnostics to stderr. In a TTY it SHALL render a table; in non-TTY or with `--json` it SHALL emit a JSON envelope carrying a `schema_version`, the `query`, a `semantic_used` boolean, a nullable `degraded` reason, and a `hits` array where each hit contains `repo`, `path`, `line`, `score`, and `snippet`. The `--limit <n>` flag SHALL bound the number of hits returned. The JSON envelope SHALL remain stable so downstream agents can depend on its shape.

#### Scenario: JSON output pipes cleanly

- **WHEN** the user runs `repograph find "<query>" --json`
- **THEN** stdout contains a single valid JSON object with `schema_version`, `query`, `semantic_used`, `degraded`, and `hits`
- **AND** no diagnostic text is written to stdout
- **AND** the output parses with `jq`

#### Scenario: Retrieval mode is machine-detectable

- **WHEN** the user runs `repograph find "<query>" --semantic --json` and semantic retrieval is unavailable (missing feature, no embeddings, or no model)
- **THEN** the JSON envelope reports `semantic_used` as `false` and a non-null `degraded` reason
- **AND** the same fallback is also noted on stderr

#### Scenario: Limit bounds results

- **WHEN** the user runs `repograph find "<query>" --limit 5`
- **THEN** at most 5 hits are returned

#### Scenario: Empty results JSON shape

- **WHEN** a query matches nothing and `--json` is set
- **THEN** stdout contains a valid JSON object whose `hits` array is empty

### Requirement: Corrupt or unreadable index is surfaced

The system SHALL detect a corrupt or unreadable index database and surface it as an error on stderr with exit code 1, rather than panicking or returning partial results silently. A missing index (never built) SHALL NOT be treated as corruption; instead `repograph find` SHALL report that no index exists and direct the user to run `repograph index`.

#### Scenario: Find before any index exists

- **WHEN** the user runs `repograph find "<query>"` before ever running `repograph index`
- **THEN** stderr explains that no index exists and to run `repograph index`
- **AND** the command exits with code 3

#### Scenario: Corrupt index database

- **WHEN** the index database is present but unreadable or corrupt
- **THEN** stderr reports the failure
- **AND** the command exits with code 1

