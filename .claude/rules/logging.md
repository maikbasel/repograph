# Logging & Observability

**Stack**: `tracing` + `tracing-subscriber` (structured diagnostics). Never use `println!` / `eprintln!` for anything beyond the explicit user-facing output that lives in `output.rs`.

## Logger Usage

- Initialize a `tracing-subscriber` once at the top of `main()`. The formatter writes to **stderr** so structured logs never pollute stdout's data contract (see CLAUDE.md output contract).
- Use `tracing::{debug, info, warn, error}` macros — never `println!` for diagnostics.
- The only legitimate stdout writers are the renderers in `output.rs` (table or JSON). Everything else goes through `tracing` to stderr.
- Domain types (`config.rs` data structs, error variants in `error.rs`) must NOT log — they are pure values. Logging belongs in command handlers (`commands/<name>.rs`), `git.rs` adapters, and `output.rs`.

## What to Log per Command

Every command's `run()` MUST log at three points:

| Point   | Level   | Fields                                  | Example                                                       |
|---------|---------|-----------------------------------------|---------------------------------------------------------------|
| Entry   | `debug` | command name + key inputs (no secrets)  | `debug!(command = "register", path = %args.path, "start");`   |
| Success | `info`  | resulting entity ID / count             | `info!(repo = %repo.name, "registered");`                     |
| Error   | `error` | the error itself + relevant context     | `error!(err = ?e, path = %args.path, "register failed");`     |

Pass errors as `err = ?e` (Debug) or `err = %e` (Display) — `tracing` records them with the chain. Always re-throw via `?`; do not swallow.

## Spans for Operation Context

Use `#[tracing::instrument]` on command `run()` functions and on `git.rs` helpers that operate on a single repo. The span carries the repo name / workspace into every nested log call automatically:

```rust
#[tracing::instrument(skip(args), fields(repo = %args.name))]
pub fn run(args: Args) -> Result<(), RepographError> { ... }
```

This means all log entries for a single operation share the same bound IDs, making the chain searchable in any structured log aggregator and trivial to follow when triaging locally.

## Log Levels

| Level   | When to use                                                                  |
|---------|------------------------------------------------------------------------------|
| `error` | Unrecoverable failures, propagated errors — always include the error itself  |
| `warn`  | Degraded state: a registered repo's path is missing, a workspace is partial  |
| `info`  | User-visible events: repo registered, workspace created, context exported   |
| `debug` | Operation entry, adapter calls, `git2` details, implementation noise         |

Default subscriber level is `info`. Honor `RUST_LOG` (e.g. `RUST_LOG=repograph=debug`) and a `--verbose` / `-v` flag for triage.

## What NOT to log

- Full file contents (large `CLAUDE.md` bodies inlined into context output) — log a length or a hash instead.
- Secrets or credentials if the project ever grows them (auth tokens for remote git, API keys) — never log them; redact at the field boundary.
- User home paths in a way that obscures the actual data being logged — paths are fine, but make sure structured fields are still useful when piped through a log aggregator.

## Reconciling with the stdout / stderr contract

The CLAUDE.md output contract is non-negotiable: stdout is pure data, stderr is everything else. `tracing` writes to stderr by construction, so structured logging slots in below the spinner / progress layer (`indicatif`) without conflict. When piping to `jq` or another agent, stderr can be captured separately or discarded — stdout stays clean.

When `indicatif` spinners are active, draw them through `tracing-indicatif` (or pause the spinner before emitting a log line) so the spinner doesn't shred multi-line tracing output. Clear all spinners before the command returns.
