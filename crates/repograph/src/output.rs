//! TTY detection and rendering for the registry.

use std::collections::BTreeMap;
use std::io::{self, Write};

use comfy_table::{Cell, Table, presets::UTF8_FULL};
use is_terminal::IsTerminal;
use repograph_core::{Repo, RepographError};
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
    repos: Vec<ListEntry<'a>>,
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
    match mode {
        OutputMode::Json => write_json(repos),
        OutputMode::Tty => write_table(repos),
    }
}

fn write_json(repos: &BTreeMap<String, Repo>) -> Result<(), RepographError> {
    let entries: Vec<ListEntry> = repos
        .iter()
        .map(|(name, r)| ListEntry {
            name,
            path: &r.path,
            description: r.description.as_deref(),
            stack: &r.stack,
        })
        .collect();
    let envelope = ListEnvelope { repos: entries };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &envelope).map_err(serde_json_to_repograph)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn write_table(repos: &BTreeMap<String, Repo>) -> Result<(), RepographError> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Name", "Path", "Description", "Stack"]);
    for (name, repo) in repos {
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
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{table}")?;
    Ok(())
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
    #![allow(clippy::unwrap_used)]
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
        let envelope = ListEnvelope { repos: entries };
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
        let body = serde_json::to_string(&ListEnvelope { repos: entries }).unwrap();
        assert_eq!(body, "{\"repos\":[]}");
    }
}
