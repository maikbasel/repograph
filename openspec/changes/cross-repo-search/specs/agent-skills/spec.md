## ADDED Requirements

### Requirement: Agent artifact teaches cross-repo find

The generated per-agent instruction artifact SHALL include guidance teaching the agent to invoke `repograph find "<query>"` when the user signals that a solution likely already exists in another repo — for example "I did this before", "this is solved somewhere", or "use repo X as reference" — including the case where the user cannot name the repo. The guidance SHALL position `repograph find` as the way to locate cross-repo precedent before re-implementing.

#### Scenario: Artifact body includes find guidance

- **WHEN** `repograph init` writes the per-agent instruction artifact
- **THEN** the artifact body contains guidance to call `repograph find` for cross-repo precedent queries
- **AND** the guidance distinguishes this from same-repo lookups
