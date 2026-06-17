## ADDED Requirements

### Requirement: Edit updates a registered repository in place

The CLI SHALL accept `repograph edit <name> [--name <new-name>] [--description <text>] [--stack <csv>] [--path <path>]` to update an existing registry entry in place, without removing and re-adding it. The target `<name>` MUST already be registered. Only the flags supplied SHALL change; omitted fields SHALL retain their current values. `--path`, when supplied, MUST point at a valid git repository (validated like `add`) and SHALL be stored canonicalized. `--name`, when supplied, renames the entry and SHALL rewrite every `workspace.members` entry that referenced the old name to the new name, so workspace groupings survive the rename with no dangling members. The change SHALL be persisted atomically.

#### Scenario: Edit changes description and stack in place

- **WHEN** a repository `foo` is registered and the user runs `repograph edit foo --description "new" --stack rust,cli`
- **THEN** the `[repo.foo]` entry's `description` becomes `"new"` and `stack` becomes `["rust", "cli"]`, the path is unchanged, the exit code is `0`, and stderr confirms the update

#### Scenario: Edit of a nonexistent name is not found

- **WHEN** no repository named `foo` is registered and the user runs `repograph edit foo --description x`
- **THEN** the exit code is `3`, stderr explains, and the config is unchanged

#### Scenario: Rename preserves workspace membership

- **WHEN** repository `foo` is a member of workspace `acme` and the user runs `repograph edit foo --name bar`
- **THEN** the registry entry is renamed to `bar`, the workspace `acme` lists `bar` (not `foo`) as a live member with no dangling reference, and the exit code is `0`

#### Scenario: Rename to an existing name conflicts

- **WHEN** repositories `foo` and `bar` are both registered and the user runs `repograph edit foo --name bar`
- **THEN** the exit code is `5`, stderr names the conflicting entry, and the config is unchanged

#### Scenario: Edit to a non-git path is not found

- **WHEN** repository `foo` is registered and the user runs `repograph edit foo --path <tempdir>/plain-dir` where the path exists but is not a git repository
- **THEN** the exit code is `3`, stderr explains, and the config is unchanged

#### Scenario: Edit with no change flags is a usage error

- **WHEN** the user runs `repograph edit foo` with none of `--name/--description/--stack/--path`
- **THEN** clap (or the command) reports a usage error on stderr and the exit code is `2`; the config is unchanged

### Requirement: Mutating registry commands emit a JSON confirmation envelope under --json

When `--json` is passed, `repograph add`, `repograph remove`, and `repograph edit` SHALL emit a single structured confirmation object to stdout describing the committed change (at least an `action` discriminator and the affected entry's resulting fields), and SHALL keep all diagnostics on stderr. Without `--json`, these commands SHALL continue to emit nothing to stdout (confirmation on stderr only), preserving the existing output contract. The confirmation SHALL be emitted only after the change is persisted, so an agent can treat its presence as verification.

#### Scenario: add --json confirms the registered entry

- **WHEN** the user runs `repograph add <tempdir>/myrepo --name foo --stack rust --json 2>/dev/null`
- **THEN** stdout is a single JSON object with `action = "add"` and the resulting entry (`name = "foo"`, canonical `path`, `stack = ["rust"]`); it parses cleanly through `jq`; exit code is `0`

#### Scenario: remove --json confirms the removed name

- **WHEN** repository `foo` is registered and the user runs `repograph remove foo --json 2>/dev/null`
- **THEN** stdout is a single JSON object with `action = "remove"` and `name = "foo"`; exit code is `0`

#### Scenario: edit --json confirms the updated entry

- **WHEN** repository `foo` is registered and the user runs `repograph edit foo --description new --json 2>/dev/null`
- **THEN** stdout is a single JSON object with `action = "edit"` and the resulting entry fields; exit code is `0`

#### Scenario: Mutators without --json keep stdout empty

- **WHEN** the user runs `repograph add <tempdir>/myrepo --name foo > /tmp/out 2> /tmp/err` (no `--json`)
- **THEN** `/tmp/out` is empty and `/tmp/err` contains the success confirmation
