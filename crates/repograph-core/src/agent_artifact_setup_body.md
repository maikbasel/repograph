## What repograph-setup is

repograph-setup is the mutating half of repograph: it registers local git repositories, groups them into workspaces, and updates existing registry entries. The read-only `repograph` skill resolves and reads the registry; this skill *changes* it. The registry is the user's, so every mutation runs through a plan → confirm → execute → verify workflow — never mutate on a guess.

## When to invoke

- The user wants to register a repo: "add this repo to repograph", "register /path/to/project", "track this project".
- The user wants to group repos: "create a workspace for the acme project", "put api and web in one workspace", "group these together".
- The user wants to change an entry: "rename that repo", "update the description", "retag it as rust,cli", "point it at the new path".
- The user wants to deregister: "remove that repo from repograph", "delete the acme workspace".

## Workflow — plan, confirm, execute, verify

Follow these steps for every mutation. Do not skip the confirmation.

1. **Plan.** Resolve the concrete inputs: the absolute path to register, the name to use (default: the path's basename), the workspace and members involved. State the exact change you are about to make.
2. **Confirm.** Show the user the plan and get explicit agreement before writing. The registry is theirs to manage; an unconfirmed mutation is a bug.
3. **Execute.** Run the command with `--json` so the result is machine-verifiable.
4. **Verify.** Read the `--json` confirmation envelope (its `action` field names the committed operation) to confirm the change landed. Reach for `repograph list --json` or `repograph workspace show <name> --json` only if you need the full post-state.

## Commands

| Intent                                   | Command                                                       |
|------------------------------------------|--------------------------------------------------------------|
| Register a repo                          | `repograph add <path> [--name N] [--description D] [--stack csv] --json` |
| Update an entry in place                 | `repograph edit <name> [--name NEW] [--description D] [--stack csv] [--path P] --json` |
| Deregister a repo                        | `repograph remove <name> --json`                             |
| Create a workspace                       | `repograph workspace create <name> [--description D] --json` |
| Attach repos to a workspace              | `repograph workspace add <workspace> <repo...> --json`       |
| Detach repos from a workspace            | `repograph workspace remove <workspace> <repo...> --json`    |
| Delete a workspace                       | `repograph workspace rm <name> --json`                       |

Always pass `--json` so the command echoes a structured confirmation to stdout; diagnostics stay on stderr. `repograph edit` is the non-lossy way to change an existing entry — prefer it over remove-then-add, which would drop the repo's workspace memberships. Renaming via `edit --name` rewrites those memberships so groupings survive.

## Things to avoid

- Do not mutate without confirming the plan first. Registration, grouping, and edits are the user's decisions to ratify.
- Do not remove-then-add to "edit" an entry — use `repograph edit`, which preserves workspace membership. A bare `remove` orphans every workspace reference to that repo.
- Do not invent a path. Resolve the real absolute path the user means before calling `add`/`edit --path`; a non-git path is rejected.
- Do not select agent toolchains here. Re-running `repograph init` to reconfigure agents is the user's call, not this skill's.
