# Production-Grade Implementations Only

repograph is a tool that real developers and AI agents rely on for accurate repository context. Every line of code, every test, every error path must assume a real user is on the other end. There is no "throwaway", no "we'll fix it later", no "good enough for demo". If something isn't ready for a real user to encounter, it does not merge.

This rule extends CLAUDE.md's "NO PARTIAL IMPLEMENTATIONS" to every artifact the repo produces.

## What this forbids

- **POC / prototype code in `master`.** If code is exploratory, it lives on a throwaway branch and is deleted, not merged "behind a flag for now".
- **`TODO`, `FIXME`, `HACK`, `XXX` comments** pointing at work we intend to come back to. Either do the work or delete the hook for it. Open a tracked issue if it's genuinely future scope, and keep the code clean now.
- **`todo!()`, `unimplemented!()`, placeholder structs, stub functions** outside of an explicit skeleton phase that the active openspec change defines and that follow-up tasks resolve in the same change.
- **Happy-path-only implementations.** Empty workspaces, missing repos, repos that were deleted out from under us, bare repos, detached HEADs, network-mounted paths, repos with no commits — all are part of the feature, not v2.
- **Silent failures.** `.unwrap()` / `.expect()` in non-test code, swallowed errors via `.ok()`, `let _ = ...` that hides a `Result`, panics in adapter code. If you can't recover, surface via `thiserror` with the right exit code.
- **Mixed stdout / stderr.** Pure data on stdout (JSON when `--json` or non-TTY, table when TTY); diagnostics, spinners, progress, warnings on stderr. No exceptions — this is a contract, not a heuristic.
- **Hardcoded paths.** Always resolve user paths via `dirs::config_dir()` / `dirs::home_dir()`. Tests write to `tempdir`, never the real config.
- **Shelling out to `git`.** Use `git2` exclusively — no `Command::new("git")`.
- **Magic strings / numbers without names.** Extract a `const` with a descriptive name.
- **Output stable for a TTY but broken when piped (or vice versa).** Both modes are first-class; both are tested.
- **Dev-only shortcuts shipped to release builds.** No `if cfg!(debug_assertions)` guards that hide missing behavior. If it works in `cargo run`, it must work in `cargo install`.

## What this requires

- **Every command is complete end-to-end before merge**: clap-derived argument parsing, config load, git2 introspection, error mapping with the correct exit code, both output modes (TTY table + `--json`), tests covering both, README-documented behavior.
- **Error paths are designed, not caught by a global `Result<()>` boundary as an afterthought.** Each failure mode maps to a documented exit code (see CLAUDE.md exit code contract) and a user-visible message on stderr.
- **Tests run against real adapters where the code owns the implementation.** Use `tempdir` + `git2` to init real repos in tests; do not mock `git2`. See `testing.md`.
- **Observability is part of the feature.** Use `tracing` for diagnostics — see `logging.md`. Every command logs entry, success, and error.
- **The happy path, the sad path, and the paranoid path all work**: empty config file, malformed TOML, a registered repo whose path no longer exists, two repos with the same name, a workspace pointing at a deleted repo, `--json` piped to `jq`, output redirected to a file, `repograph` invoked from inside a non-git directory.

## Rationale

repograph is consumed by AI agents that build on its output. A flaky exit code, an unstable JSON shape, a swallowed error — those become silent failures in the calling agent that are an order of magnitude harder to debug than the original bug would have been. The cost of doing it right the first time is always lower than the cost of debugging a downstream agent that's quietly drifting because repograph half-failed.

## If you're tempted to ship something half-done

Ask instead: *"What would I need to do to make this actually production-ready?"* Then scope the work honestly. If it's too big for this change, cut the surface area — ship less, but ship it done. Never ship more, done halfway.
