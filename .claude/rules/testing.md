---
paths:
  - "**/tests/**/*.rs"
  - "**/*test*.rs"
  - "**/*spec*.rs"
  - "Cargo.toml"
---

# Testing Philosophy (Outside-In TDD)

## Approach

Start with an acceptance test that describes the full observable behavior of a CLI command — exit code, stdout shape (JSON or table), stderr message. Let it stay red. Each failure message tells you what to build next. Drop into a unit test only when you need fast design feedback on a specific seam. When all pieces are in place, the acceptance test goes green — done.

## The Inner Loop

Each step in the implementation order runs this micro-cycle:

1. Run the acceptance test — observe the first failing assertion
2. Write the smallest focused unit test that targets that specific seam (config deserialization, git2 helper, output rendering, error mapping) — it must be red
3. Implement just enough to make that focused test green
4. Refactor if needed
5. Re-run the acceptance test — move to the next failure and repeat

**Tooling for adapter seams**: when the seam touches `git2`, build a real repo in a `tempdir` with `git2` itself. Never mock `git2` — mocks drift from libgit2's actual behavior and bugs hide in that gap.

**Focused tests are disposable**: some focused tests exist only to drive the design of a seam while the acceptance test is still red. Once the acceptance test goes green, delete any focused test whose assertion is fully covered by the acceptance test. Coverage is a side-effect, not the goal.

## Acceptance Tests (`assert_cmd`)

- Test the binary as a real user would invoke it: spawn it via `assert_cmd::Command`, assert exit code, stdout JSON shape, stderr message
- Stay red until the complete feature works end-to-end — that is expected and correct
- Drive top-down: the failing acceptance test tells you which layer to build next
- Use `tempdir` for any filesystem state — never touch `~/.config/repograph/`
- Test both output modes: `--json` (machine, parsed via `serde_json` in the test) and TTY (human, snapshot or contains-assertions on the table)
- Exit codes are part of the assertion — `.code(3)` for not-found, `.code(5)` for conflict, etc. (see CLAUDE.md exit code contract)

## Unit Tests (`cargo test`)

- Triggered by a *new requirement*, not a code change
- Test behavior of a unit (config round-trip, git status mapping, output mode selection) — not its implementation details
- Used for fast feedback on a specific design seam, not for coverage
- Co-located with the module under test (`#[cfg(test)] mod tests { ... }`) when the seam is internal; promoted to `tests/` only for cross-module integration

## Bug Fixes — Reproduce First

When resolving a bug, **always write a failing test that reproduces the behavior before touching production code**. The test locks in the expected behavior, proves the fix actually fixes *this* bug (not a similar one), and guards against regression. No exceptions — even "obvious" one-line fixes get a reproducing test.

- Start with the highest-level test that can reproduce the issue (acceptance > unit).
- Run the test and confirm it fails **for the reason you expect**. A test that fails for the wrong reason is worse than no test.
- Only then write the fix. The test must turn green without loosening its assertions.
- If a bug cannot be reproduced in a test, stop and ask why — untestable bugs are design smells.

## Test Resilience Rule

A change in implementation must not break a test as long as the requirement is still fulfilled. If it does, the test is testing implementation, not behavior — fix the test.

## Anti-Patterns to Avoid

- Testing implementation details (private fns, internal struct fields)
- Writing tests after the fact to hit coverage numbers
- Mocking at the application boundary when a real adapter (`git2` + `tempdir`) is fast
- One test per function (test behaviors and outcomes instead)
- Touching `~/.config/repograph/` from a test — always `tempdir`, with `REPOGRAPH_CONFIG_DIR` (or equivalent) overridden if needed
- Asserting on full stderr text including timestamps or paths — assert on stable substrings or use `predicates::str::contains`
