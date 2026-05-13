//! TTY detection and rendering for the registry.

use std::collections::BTreeMap;
use std::io::{self, Write};

use comfy_table::{Cell, Table, presets::UTF8_FULL};
use is_terminal::IsTerminal;
use repograph_core::{Repo, RepographError, Workspace};
use serde::Serialize;

/// Decided once at command entry; passed down so renderers never re-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Tty,
    Json,
}

impl OutputMode {
    /// Choose between `Json` (when `force_json` or stdout is not a TTY) and
    /// `Tty` (when stdout is an interactive terminal).
    pub fn detect(force_json: bool) -> Self {
        if force_json || !io::stdout().is_terminal() {
            Self::Json
        } else {
            Self::Tty
        }
    }
}

#[derive(Serialize)]
struct ListEntry<'a> {
    name: &'a str,
    path: &'a std::path::Path,
    description: Option<&'a str>,
    stack: &'a [String],
}

#[derive(Serialize)]
struct ListEnvelope<'a> {
    repos: &'a [ListEntry<'a>],
}

/// Render the registered repositories. Writes a `comfy-table` rendering to
/// stdout when `mode == Tty`; emits a `{ "repos": [...] }` JSON envelope when
/// `mode == Json`. Diagnostics never reach stdout from this path.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_repos(
    mode: OutputMode,
    repos: &BTreeMap<String, Repo>,
) -> Result<(), RepographError> {
    let entries: Vec<ListEntry> = repos
        .iter()
        .map(|(name, r)| ListEntry {
            name,
            path: &r.path,
            description: r.description.as_deref(),
            stack: &r.stack,
        })
        .collect();
    render_repo_entries(mode, &entries)
}

/// Render a pre-filtered slice of repositories, used by `list --workspace`
/// where the caller has already resolved the workspace into live members.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_repo_slice(
    mode: OutputMode,
    repos: &[(&String, &Repo)],
) -> Result<(), RepographError> {
    let entries: Vec<ListEntry> = repos
        .iter()
        .map(|(name, r)| ListEntry {
            name: name.as_str(),
            path: &r.path,
            description: r.description.as_deref(),
            stack: &r.stack,
        })
        .collect();
    render_repo_entries(mode, &entries)
}

fn render_repo_entries(mode: OutputMode, entries: &[ListEntry<'_>]) -> Result<(), RepographError> {
    match mode {
        OutputMode::Json => write_repo_json(entries),
        OutputMode::Tty => write_repo_table(entries),
    }
}

fn write_repo_json(entries: &[ListEntry<'_>]) -> Result<(), RepographError> {
    let envelope = ListEnvelope { repos: entries };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &envelope).map_err(serde_json_to_repograph)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn write_repo_table(entries: &[ListEntry<'_>]) -> Result<(), RepographError> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Name", "Path", "Description", "Stack"]);
    for entry in entries {
        table.add_row(vec![
            Cell::new(entry.name),
            Cell::new(entry.path.display()),
            Cell::new(entry.description.unwrap_or("-")),
            Cell::new(if entry.stack.is_empty() {
                String::from("-")
            } else {
                entry.stack.join(", ")
            }),
        ]);
    }
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{table}")?;
    Ok(())
}

#[derive(Serialize)]
struct WorkspaceListEntry<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    members: &'a [String],
}

#[derive(Serialize)]
struct WorkspaceListEnvelope<'a> {
    workspaces: Vec<WorkspaceListEntry<'a>>,
}

#[derive(Serialize)]
struct WorkspaceShowEnvelope<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    members: Vec<ListEntry<'a>>,
    dangling: Vec<&'a str>,
}

/// Render the registered workspaces. TTY rendering shows name, description,
/// and member count; JSON emits a `{ "workspaces": [...] }` envelope. Members
/// are presented verbatim from the stored array (including any tombstoned
/// names) — `workspace ls` is a metadata view, not a resolved view.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_workspaces(
    mode: OutputMode,
    workspaces: &BTreeMap<String, Workspace>,
) -> Result<(), RepographError> {
    match mode {
        OutputMode::Json => write_workspace_list_json(workspaces),
        OutputMode::Tty => write_workspace_list_table(workspaces),
    }
}

fn write_workspace_list_json(
    workspaces: &BTreeMap<String, Workspace>,
) -> Result<(), RepographError> {
    let entries: Vec<WorkspaceListEntry> = workspaces
        .iter()
        .map(|(name, ws)| WorkspaceListEntry {
            name,
            description: ws.description.as_deref(),
            members: &ws.members,
        })
        .collect();
    let envelope = WorkspaceListEnvelope {
        workspaces: entries,
    };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &envelope).map_err(serde_json_to_repograph)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn write_workspace_list_table(
    workspaces: &BTreeMap<String, Workspace>,
) -> Result<(), RepographError> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Name", "Description", "Members"]);
    for (name, ws) in workspaces {
        table.add_row(vec![
            Cell::new(name),
            Cell::new(ws.description.as_deref().unwrap_or("-")),
            Cell::new(ws.members.len()),
        ]);
    }
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{table}")?;
    Ok(())
}

/// Render a single workspace's details. TTY emits a table of live members;
/// JSON emits a `{ name, description, members: [...], dangling: [...] }`
/// envelope where `dangling` is ALWAYS present (empty array when none).
/// The caller is responsible for any per-member dangling stderr warnings.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_workspace_show(
    mode: OutputMode,
    name: &str,
    description: Option<&str>,
    live: &[(&String, &Repo)],
    dangling: &[&String],
) -> Result<(), RepographError> {
    let live_entries: Vec<ListEntry> = live
        .iter()
        .map(|(member_name, r)| ListEntry {
            name: member_name.as_str(),
            path: &r.path,
            description: r.description.as_deref(),
            stack: &r.stack,
        })
        .collect();
    let dangling_refs: Vec<&str> = dangling.iter().map(|s| s.as_str()).collect();
    match mode {
        OutputMode::Json => {
            let envelope = WorkspaceShowEnvelope {
                name,
                description,
                members: live_entries,
                dangling: dangling_refs,
            };
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &envelope).map_err(serde_json_to_repograph)?;
            stdout.write_all(b"\n")?;
            Ok(())
        }
        OutputMode::Tty => {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(vec!["Name", "Path", "Description", "Stack"]);
            for entry in &live_entries {
                table.add_row(vec![
                    Cell::new(entry.name),
                    Cell::new(entry.path.display()),
                    Cell::new(entry.description.unwrap_or("-")),
                    Cell::new(if entry.stack.is_empty() {
                        String::from("-")
                    } else {
                        entry.stack.join(", ")
                    }),
                ]);
            }
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{table}")?;
            Ok(())
        }
    }
}

fn serde_json_to_repograph(e: serde_json::Error) -> RepographError {
    if e.is_io() {
        RepographError::Io(e.into())
    } else {
        // Logical/serialization failures shouldn't happen for our static shape;
        // surface them as general I/O so the user sees something rather than panic.
        RepographError::Io(io::Error::other(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> BTreeMap<String, Repo> {
        let mut map = BTreeMap::new();
        map.insert(
            "alpha".to_string(),
            Repo {
                path: PathBuf::from("/tmp/alpha"),
                description: Some("first".into()),
                stack: vec!["rust".into(), "cli".into()],
            },
        );
        map.insert(
            "beta".to_string(),
            Repo {
                path: PathBuf::from("/tmp/beta"),
                description: None,
                stack: vec![],
            },
        );
        map
    }

    /// TTY rendering goes through `write_table` directly so it is testable
    /// without an actual terminal. Acceptance tests cover the JSON path.
    #[test]
    fn tty_rendering_includes_headers_and_rows() {
        let repos = fixture();
        // Build the table the same way `write_table` does, then assert structure.
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec!["Name", "Path", "Description", "Stack"]);
        for (name, repo) in &repos {
            table.add_row(vec![
                Cell::new(name),
                Cell::new(repo.path.display()),
                Cell::new(repo.description.as_deref().unwrap_or("-")),
                Cell::new(if repo.stack.is_empty() {
                    String::from("-")
                } else {
                    repo.stack.join(", ")
                }),
            ]);
        }
        let rendered = table.to_string();

        assert!(rendered.contains("Name"), "header rendered");
        assert!(rendered.contains("Path"));
        assert!(rendered.contains("Description"));
        assert!(rendered.contains("Stack"));
        assert!(rendered.contains("alpha"));
        assert!(rendered.contains("beta"));
        assert!(rendered.contains("/tmp/alpha"));
        assert!(rendered.contains("rust, cli"));
        assert!(rendered.contains("first"));
        assert!(rendered.contains('-'), "empty fields render as dash");
    }

    #[test]
    fn json_envelope_shape() {
        let repos = fixture();
        let entries: Vec<ListEntry> = repos
            .iter()
            .map(|(name, r)| ListEntry {
                name,
                path: &r.path,
                description: r.description.as_deref(),
                stack: &r.stack,
            })
            .collect();
        let envelope = ListEnvelope { repos: &entries };
        let body = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["repos"].is_array());
        assert_eq!(v["repos"].as_array().unwrap().len(), 2);
        assert_eq!(v["repos"][0]["name"], "alpha");
        assert_eq!(v["repos"][0]["stack"][0], "rust");
        assert_eq!(v["repos"][1]["name"], "beta");
        assert!(v["repos"][1]["description"].is_null());
    }

    #[test]
    fn empty_registry_json_is_empty_array() {
        let repos: BTreeMap<String, Repo> = BTreeMap::new();
        let entries: Vec<ListEntry> = repos
            .iter()
            .map(|(name, r)| ListEntry {
                name,
                path: &r.path,
                description: r.description.as_deref(),
                stack: &r.stack,
            })
            .collect();
        let body = serde_json::to_string(&ListEnvelope { repos: &entries }).unwrap();
        assert_eq!(body, "{\"repos\":[]}");
    }

    fn workspace_fixture() -> BTreeMap<String, Workspace> {
        let mut map = BTreeMap::new();
        map.insert(
            "acme".into(),
            Workspace {
                description: Some("Acme rebuild".into()),
                members: vec!["api".into(), "ui".into()],
            },
        );
        map.insert(
            "billing".into(),
            Workspace {
                description: None,
                members: vec![],
            },
        );
        map
    }

    #[test]
    fn workspace_list_json_envelope_shape() {
        let workspaces = workspace_fixture();
        let entries: Vec<WorkspaceListEntry> = workspaces
            .iter()
            .map(|(name, ws)| WorkspaceListEntry {
                name,
                description: ws.description.as_deref(),
                members: &ws.members,
            })
            .collect();
        let body = serde_json::to_string(&WorkspaceListEnvelope {
            workspaces: entries,
        })
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let arr = v["workspaces"].as_array().expect("workspaces array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "acme");
        assert_eq!(arr[0]["description"], "Acme rebuild");
        assert_eq!(arr[0]["members"][0], "api");
        assert_eq!(arr[1]["name"], "billing");
        // description omitted (skip_serializing_if).
        assert!(arr[1].get("description").is_none());
        assert_eq!(arr[1]["members"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn empty_workspaces_json_is_empty_array() {
        let workspaces: BTreeMap<String, Workspace> = BTreeMap::new();
        let entries: Vec<WorkspaceListEntry> = workspaces
            .iter()
            .map(|(name, ws)| WorkspaceListEntry {
                name,
                description: ws.description.as_deref(),
                members: &ws.members,
            })
            .collect();
        let body = serde_json::to_string(&WorkspaceListEnvelope {
            workspaces: entries,
        })
        .unwrap();
        assert_eq!(body, "{\"workspaces\":[]}");
    }

    #[test]
    fn workspace_show_envelope_always_has_dangling_field() {
        use std::path::PathBuf;

        let api_name = String::from("api");
        let api = Repo {
            path: PathBuf::from("/tmp/api"),
            description: None,
            stack: vec![],
        };
        let ghost_name = String::from("ghost");
        let live: Vec<(&String, &Repo)> = vec![(&api_name, &api)];
        let dangling: Vec<&String> = vec![&ghost_name];

        let live_entries: Vec<ListEntry> = live
            .iter()
            .map(|(name, r)| ListEntry {
                name: name.as_str(),
                path: &r.path,
                description: r.description.as_deref(),
                stack: &r.stack,
            })
            .collect();
        let dangling_refs: Vec<&str> = dangling.iter().map(|s| s.as_str()).collect();
        let envelope = WorkspaceShowEnvelope {
            name: "acme",
            description: Some("rebuild"),
            members: live_entries,
            dangling: dangling_refs,
        };
        let body = serde_json::to_string(&envelope).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["name"], "acme");
        assert_eq!(v["description"], "rebuild");
        assert_eq!(v["members"].as_array().unwrap().len(), 1);
        assert_eq!(v["members"][0]["name"], "api");
        let d = v["dangling"].as_array().expect("dangling is an array");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0], "ghost");
    }

    #[test]
    fn workspace_show_envelope_empty_dangling_is_array_not_null() {
        let live: Vec<(&String, &Repo)> = vec![];
        let dangling: Vec<&String> = vec![];
        let live_entries: Vec<ListEntry> = live
            .iter()
            .map(|(name, r)| ListEntry {
                name: name.as_str(),
                path: &r.path,
                description: r.description.as_deref(),
                stack: &r.stack,
            })
            .collect();
        let dangling_refs: Vec<&str> = dangling.iter().map(|s| s.as_str()).collect();
        let envelope = WorkspaceShowEnvelope {
            name: "empty",
            description: None,
            members: live_entries,
            dangling: dangling_refs,
        };
        let body = serde_json::to_string(&envelope).unwrap();
        // No `description` key (skipped) but `dangling` MUST be `[]`, not absent, not null.
        assert!(body.contains("\"dangling\":[]"), "got body: {body}");
        assert!(!body.contains("description"), "description omitted; body: {body}");
    }
}
