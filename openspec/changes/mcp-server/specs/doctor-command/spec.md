## ADDED Requirements

### Requirement: Doctor reports MCP registration health

The doctor check catalog SHALL gain a check that reports, for each agent selected in config that hosts MCP, whether a `repograph` MCP server registration exists and whether the command it points at resolves to an executable file. The check SHALL emit one finding per selected MCP-hosting agent. Agents that host no MCP SHALL NOT produce a finding.

Severity SHALL be: `ok` when a registration exists and its command resolves; `warn` when no registration exists for a selected MCP-hosting agent; `error` when a registration exists but its command does not resolve to an executable.

#### Scenario: Registered and resolvable reports ok

- **WHEN** `repograph doctor --json` runs with `cursor` selected and a `repograph` entry in the Cursor config whose command resolves
- **THEN** a finding for `cursor` is present with severity `ok`

#### Scenario: Missing registration reports warn

- **WHEN** `repograph doctor --json` runs with `cursor` selected and no `repograph` entry in the Cursor config
- **THEN** a finding for `cursor` is present with severity `warn`, and the message names the command that would fix it

#### Scenario: Stale binary path reports error

- **WHEN** a registration exists whose command points at a path that is absent or not executable
- **THEN** the finding has severity `error` and names the unresolvable path

#### Scenario: Non-MCP agents produce no finding

- **WHEN** `repograph doctor --json` runs with only `aider` selected
- **THEN** no MCP registration finding appears in the check catalog

#### Scenario: The new check participates in the summary and exit code

- **WHEN** the MCP registration check yields an `error` finding and no other check errors
- **THEN** the `summary` block counts it and the process exits with the documented error exit code

#### Scenario: JSON envelope stays stable

- **WHEN** `repograph doctor --json` is parsed
- **THEN** `schema_version` is unchanged and the new finding uses the existing `check` / `severity` / `target` / `message` shape without introducing new top-level fields
