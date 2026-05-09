---
paths:
  - "openspec/**/*"
  - "README.md"
  - "**/design.md"
  - "**/proposal.md"
  - "**/tasks.md"
---

# Living Documentation

The active change's `proposal.md` / `design.md` / `tasks.md` / `specs/`, the rolled-up `openspec/specs/`, and `README.md` are living artifacts. Keep them in sync with what ships.

## When to update

- **`design.md` (active change)** — update the moment an implementation decision deviates from the plan. The design doc must reflect what was actually built before the change is archived. If the deviation is significant, add a "Resolved deviation" note explaining why.
- **`tasks.md` (active change)** — tick boxes as work completes. Don't tick a box for code that doesn't compile, whose tests are red, or whose error paths aren't wired.
- **`specs/` (active change)** — new behavior gets a spec entry before the implementation. If the behavior changes mid-implementation, update the spec, then continue.
- **`README.md`** — update the exit code table, command surface, install instructions, and example output when any of them change. Users read this first; it's the canonical contract for the binary.

## What NOT to update retroactively

- Don't rewrite history. If a decision changed, add a "Resolved deviation" note explaining why, rather than silently replacing the original intent.
- Don't update docs to match broken or incomplete implementations — fix the implementation first, then the doc.
- Don't hand-edit `CHANGELOG.md`, the `version` field in `Cargo.toml`, or `.github/workflows/release.yml` — these are owned by Release Please and cargo-dist respectively (see CLAUDE.md). Hand edits are clobbered on the next release.
- Don't archive a change before its `tasks.md` is fully ticked and tests pass.

## Checklist before archiving a change

- [ ] All tasks in `tasks.md` are ticked
- [ ] `cargo check` passes with zero warnings
- [ ] `cargo clippy -- -D warnings` is clean
- [ ] `cargo test` passes
- [ ] Exit codes match the contract table in `README.md` and CLAUDE.md
- [ ] `--json` output is valid, parseable JSON for every command the change touched
- [ ] No `unwrap()` / `expect()` outside test code
- [ ] `design.md` reflects what was actually built (resolved deviations noted)
- [ ] `README.md`'s command surface and exit code table are current
