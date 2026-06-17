## ADDED Requirements

### Requirement: Workspace mutating commands emit a JSON confirmation envelope under --json

When `--json` is passed, `repograph workspace create`, `repograph workspace add`, `repograph workspace remove`, and `repograph workspace rm` SHALL emit a single structured confirmation object to stdout describing the committed change (at least an `action` discriminator, the workspace name, and — for member operations — the affected repo names), and SHALL keep all diagnostics, dangling warnings, and confirmations on stderr. Without `--json`, these commands SHALL continue to emit nothing to stdout (confirmation on stderr only), preserving the existing workspace output contract. The confirmation SHALL be emitted only after the change is persisted.

#### Scenario: workspace create --json confirms the new workspace

- **WHEN** the user runs `repograph workspace create acme --json 2>/dev/null`
- **THEN** stdout is a single JSON object with `action = "workspace.create"` and `workspace = "acme"`; it parses through `jq`; exit code is `0`

#### Scenario: workspace add --json confirms attached members

- **WHEN** repositories `api` and `web` are registered and the user runs `repograph workspace add acme api web --json 2>/dev/null`
- **THEN** stdout is a single JSON object with `action = "workspace.add"`, `workspace = "acme"`, and the attached repo names; exit code is `0`

#### Scenario: workspace rm --json confirms deletion

- **WHEN** workspace `acme` exists and the user runs `repograph workspace rm acme --json 2>/dev/null`
- **THEN** stdout is a single JSON object with `action = "workspace.rm"` and `workspace = "acme"`; registered repos are untouched; exit code is `0`

#### Scenario: Workspace mutators without --json keep stdout empty

- **WHEN** the user runs `repograph workspace create acme > /tmp/out 2> /tmp/err` (no `--json`)
- **THEN** `/tmp/out` is empty and `/tmp/err` contains the success confirmation
