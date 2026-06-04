//! TTY detection and rendering for the registry.

use std::collections::BTreeMap;
use std::io::{self, Write};

use comfy_table::{Cell, Color, Table, presets::UTF8_FULL};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use is_terminal::IsTerminal;
use rayon::prelude::*;
use repograph_core::{
    Check, Context, DoctorReport, Repo, RepoContext, RepoStatus, RepographError, Scope, Severity,
    Workspace,
};
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

#[derive(Serialize)]
struct StatusEnvelope<'a> {
    repos: &'a [RepoStatus],
}

/// Render per-repo status entries. TTY mode produces a `comfy-table` with the
/// documented columns; JSON mode produces a `{ "repos": [...] }` envelope
/// where every entry includes an explicit `error` field (null on healthy rows).
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_statuses(mode: OutputMode, statuses: &[RepoStatus]) -> Result<(), RepographError> {
    match mode {
        OutputMode::Json => write_status_json(statuses),
        OutputMode::Tty => write_status_table(statuses),
    }
}

fn write_status_json(statuses: &[RepoStatus]) -> Result<(), RepographError> {
    let envelope = StatusEnvelope { repos: statuses };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &envelope).map_err(serde_json_to_repograph)?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn write_status_table(statuses: &[RepoStatus]) -> Result<(), RepographError> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        "Name", "Branch", "Upstream", "Ahead", "Behind", "Dirty", "State",
    ]);
    for s in statuses {
        table.add_row(vec![
            Cell::new(&s.name),
            Cell::new(s.branch.as_deref().unwrap_or("-")),
            Cell::new(s.upstream.as_deref().unwrap_or("-")),
            Cell::new(s.ahead),
            Cell::new(s.behind),
            Cell::new(if s.dirty { "yes" } else { "no" }),
            Cell::new(state_label(s.state)),
        ]);
    }
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{table}")?;
    Ok(())
}

const fn state_label(state: repograph_core::RepoState) -> &'static str {
    use repograph_core::RepoState;
    match state {
        RepoState::Clean => "clean",
        RepoState::Dirty => "dirty",
        RepoState::Detached => "detached",
        RepoState::Unborn => "unborn",
        RepoState::Bare => "bare",
        RepoState::Missing => "missing",
    }
}

/// Render a `Context` payload to stdout. JSON mode emits a single-line
/// `serde_json` payload (no trailing newline — `repograph context | jq .`
/// works either way, but agents consuming a stream get one record per
/// invocation). TTY mode emits a Markdown document with one section per repo
/// and the same data as the JSON payload (no truncation).
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_context(mode: OutputMode, context: &Context) -> Result<(), RepographError> {
    match mode {
        OutputMode::Json => render_context_json(context, &mut io::stdout().lock()),
        OutputMode::Tty => render_context_markdown(context, &mut io::stdout().lock()),
    }
}

/// JSON renderer split out for unit testability — writes the envelope verbatim
/// and (deliberately) NO trailing newline so a `--json` invocation pipes
/// cleanly into a JSON-aware consumer that doesn't tolerate trailing
/// whitespace.
fn render_context_json<W: Write>(context: &Context, writer: &mut W) -> Result<(), RepographError> {
    serde_json::to_writer(&mut *writer, context).map_err(serde_json_to_repograph)?;
    Ok(())
}

/// Markdown renderer split out for unit testability. Writes the same data the
/// JSON path carries, in a structure paste-ready for chat clients that render
/// Markdown (Claude, `ChatGPT`, GitHub, etc.).
fn render_context_markdown<W: Write>(
    context: &Context,
    writer: &mut W,
) -> Result<(), RepographError> {
    let scope_phrase = scope_phrase(&context.scope);
    writeln!(
        writer,
        "# repograph context — {scope_phrase} ({} repo{}, {} agent{})",
        context.repos.len(),
        if context.repos.len() == 1 { "" } else { "s" },
        context.agents.len(),
        if context.agents.len() == 1 { "" } else { "s" },
    )?;
    writeln!(writer)?;

    for warning in &context.warnings {
        writeln!(writer, "> **warning:** {warning}")?;
        writeln!(writer)?;
    }

    for repo in &context.repos {
        render_repo_markdown(writer, repo)?;
    }
    Ok(())
}

fn scope_phrase(scope: &Scope) -> String {
    match scope {
        Scope::All => "all registered repos".to_string(),
        Scope::Workspace { name } => format!("workspace `{name}`"),
        Scope::Repos { repos } => {
            let mut s = String::from("repos ");
            for (i, name) in repos.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push('`');
                s.push_str(name);
                s.push('`');
            }
            s
        }
    }
}

fn render_repo_markdown<W: Write>(
    writer: &mut W,
    repo: &RepoContext,
) -> Result<(), RepographError> {
    let branch_label = repo.branch.as_deref().unwrap_or("none");
    writeln!(writer, "## {}  (branch: {branch_label})", repo.name)?;
    writeln!(writer)?;
    writeln!(writer, "`{}`", repo.path.display())?;
    writeln!(writer)?;

    for warning in &repo.warnings {
        writeln!(writer, "> **warning:** {warning}")?;
        writeln!(writer)?;
    }

    for doc in &repo.agent_docs {
        if doc.files.is_empty() {
            continue;
        }
        writeln!(writer, "### {}", doc.agent.as_str())?;
        writeln!(writer)?;
        for file in &doc.files {
            let path_str = file
                .path
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            writeln!(writer, "#### {path_str} ({})", human_size(file.bytes))?;
            writeln!(writer)?;
            let fence = pick_fence(&file.content);
            writeln!(writer, "{fence}")?;
            writer.write_all(file.content.as_bytes())?;
            if !file.content.ends_with('\n') {
                writeln!(writer)?;
            }
            writeln!(writer, "{fence}")?;
            writeln!(writer)?;
        }
    }
    Ok(())
}

/// Pick a code-fence string that won't be terminated by `content`. Default is
/// triple-backtick; falls back to triple-tilde when the content contains a
/// triple-backtick line (e.g. CLAUDE.md files that embed Markdown samples).
fn pick_fence(content: &str) -> &'static str {
    if content
        .lines()
        .any(|line| line.trim_start().starts_with("```"))
    {
        "~~~"
    } else {
        "```"
    }
}

/// Human-readable byte size: `"123 B"`, `"1.2 KB"`, `"4.5 MB"`. Single decimal
/// place above `1024`. Below 1024, integer bytes for precision.
#[allow(clippy::cast_precision_loss)]
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

/// Run `body` over each item in `items` in parallel via `rayon`. When the
/// `mode` is `Tty`, an `indicatif::MultiProgress` shows one spinner per item
/// on stderr; the `MultiProgress` is dropped (clearing the spinners) before
/// this function returns. In non-TTY mode no spinners are drawn — `body` runs
/// in parallel with no UI.
pub fn with_progress<T, R, F, B>(mode: OutputMode, items: &[T], label: F, body: B) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> String + Sync,
    B: Fn(&T) -> R + Sync + Send,
{
    match mode {
        OutputMode::Json => items.par_iter().map(body).collect(),
        OutputMode::Tty => {
            let progress = MultiProgress::new();
            let style = ProgressStyle::with_template("{spinner} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner());
            let bars: Vec<ProgressBar> = items
                .iter()
                .map(|item| {
                    let pb = progress.add(ProgressBar::new_spinner());
                    pb.set_style(style.clone());
                    pb.set_message(label(item));
                    pb.enable_steady_tick(std::time::Duration::from_millis(80));
                    pb
                })
                .collect();
            let results: Vec<R> = items
                .par_iter()
                .zip(bars.par_iter())
                .map(|(item, bar)| {
                    let r = body(item);
                    bar.finish_and_clear();
                    r
                })
                .collect();
            drop(progress);
            results
        }
    }
}

/// Render a [`DoctorReport`] to stdout. JSON mode emits a single-line
/// envelope (no trailing newline); TTY mode emits a `comfy-table` summary
/// followed by a `<N> ok · <M> warn · <K> error` footer with severity
/// colouring.
///
/// # Errors
///
/// Returns [`RepographError::Io`] when writing to stdout fails.
pub fn render_doctor(mode: OutputMode, report: &DoctorReport) -> Result<(), RepographError> {
    match mode {
        OutputMode::Json => render_doctor_json(report, &mut io::stdout().lock()),
        OutputMode::Tty => render_doctor_table(report, &mut io::stdout().lock()),
    }
}

fn render_doctor_json<W: Write>(
    report: &DoctorReport,
    writer: &mut W,
) -> Result<(), RepographError> {
    serde_json::to_writer(&mut *writer, report).map_err(serde_json_to_repograph)?;
    Ok(())
}

fn render_doctor_table<W: Write>(
    report: &DoctorReport,
    writer: &mut W,
) -> Result<(), RepographError> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec!["Severity", "Check", "Target", "Message"]);
    for f in &report.checks {
        table.add_row(vec![
            Cell::new(severity_label(f.severity)).fg(severity_colour(f.severity)),
            Cell::new(check_label(f.check)),
            Cell::new(&f.target),
            Cell::new(&f.message),
        ]);
    }
    writeln!(writer, "{table}")?;
    writeln!(
        writer,
        "{ok} ok · {warn} warn · {error} error",
        ok = report.summary.ok,
        warn = report.summary.warn,
        error = report.summary.error,
    )?;
    Ok(())
}

const fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Ok => "ok",
        Severity::Warn => "warn",
        Severity::Error => "error",
    }
}

const fn severity_colour(s: Severity) -> Color {
    match s {
        Severity::Ok => Color::Green,
        Severity::Warn => Color::Yellow,
        Severity::Error => Color::Red,
    }
}

const fn check_label(c: Check) -> &'static str {
    match c {
        Check::ConfigPresent => "ConfigPresent",
        Check::ConfigParse => "ConfigParse",
        Check::AgentsConfigured => "AgentsConfigured",
        Check::ProjectsRootExists => "ProjectsRootExists",
        Check::RepoPathExists => "RepoPathExists",
        Check::RepoIsGitRepo => "RepoIsGitRepo",
        Check::WorkspaceMembersResolve => "WorkspaceMembersResolve",
        Check::AgentDocPresent => "AgentDocPresent",
    }
}

/// Render the outcome of a `repograph update` run to `writer` (stderr in
/// practice — the command has no stdout data payload).
///
/// # Errors
///
/// Returns [`io::Error`] if the write fails.
pub fn render_update_outcome<W: Write>(
    writer: &mut W,
    outcome: &crate::selfupdate::UpdateOutcome,
) -> io::Result<()> {
    use crate::selfupdate::UpdateOutcome;
    let current = env!("CARGO_PKG_VERSION");
    match outcome {
        UpdateOutcome::Updated { from: Some(from), to } => {
            writeln!(writer, "Updated repograph {from} → {to}.")
        }
        UpdateOutcome::Updated { from: None, to } => {
            writeln!(writer, "Updated repograph to {to}.")
        }
        UpdateOutcome::AlreadyCurrent => {
            writeln!(writer, "repograph {current} is already the latest version.")
        }
        UpdateOutcome::UpdateAvailable { latest } => writeln!(
            writer,
            "repograph {latest} is available (you have {current}). Run `repograph update` to install it."
        ),
        UpdateOutcome::DeferToPackageManager => writeln!(
            writer,
            "repograph was installed via a package manager — update with `brew upgrade repograph` or `cargo install repograph`."
        ),
    }
}

/// Render the passive update notice to `writer` (always stderr in practice).
/// A single line naming the available and running versions and how to upgrade.
///
/// # Errors
///
/// Returns [`io::Error`] if the write fails.
pub fn render_update_notice<W: Write>(
    writer: &mut W,
    current: &str,
    latest: &str,
) -> io::Result<()> {
    writeln!(
        writer,
        "repograph {latest} available (you have {current}) — run `repograph update` or upgrade via your package manager."
    )
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

    fn outcome_line(outcome: &crate::selfupdate::UpdateOutcome) -> String {
        let mut buf = Vec::new();
        render_update_outcome(&mut buf, outcome).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn outcome_updated_with_prior_version_names_both() {
        let s = outcome_line(&crate::selfupdate::UpdateOutcome::Updated {
            from: Some("0.2.1".into()),
            to: "0.3.0".into(),
        });
        assert!(s.contains("0.2.1") && s.contains("0.3.0"), "{s}");
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn outcome_updated_without_prior_version_names_target() {
        let s = outcome_line(&crate::selfupdate::UpdateOutcome::Updated {
            from: None,
            to: "0.3.0".into(),
        });
        assert!(s.contains("0.3.0"), "{s}");
    }

    #[test]
    fn outcome_already_current_says_latest() {
        let s = outcome_line(&crate::selfupdate::UpdateOutcome::AlreadyCurrent);
        assert!(s.contains(env!("CARGO_PKG_VERSION")), "{s}");
        assert!(s.contains("latest"), "{s}");
    }

    #[test]
    fn outcome_update_available_points_at_update_command() {
        let s = outcome_line(&crate::selfupdate::UpdateOutcome::UpdateAvailable {
            latest: "0.3.0".into(),
        });
        assert!(s.contains("0.3.0") && s.contains("repograph update"), "{s}");
    }

    #[test]
    fn outcome_defer_names_both_package_managers() {
        let s = outcome_line(&crate::selfupdate::UpdateOutcome::DeferToPackageManager);
        assert!(s.contains("brew upgrade repograph"), "{s}");
        assert!(s.contains("cargo install repograph"), "{s}");
    }

    #[test]
    fn update_notice_names_versions_and_command() {
        let mut buf = Vec::new();
        render_update_notice(&mut buf, "0.2.1", "0.3.0").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("0.3.0"), "names the available version: {s}");
        assert!(s.contains("0.2.1"), "names the running version: {s}");
        assert!(
            s.contains("repograph update"),
            "points at the update command: {s}"
        );
        assert!(s.ends_with('\n'), "is a single terminated line: {s:?}");
    }

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

    // ─── Context renderers ────────────────────────────────────────────────

    fn synth_context(scope: Scope, content: &str) -> Context {
        use repograph_core::{AgentDoc, AgentId, MatchedFile, RepoContext, SCHEMA_VERSION};
        Context {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-05-24T00:00:00Z".into(),
            agents: vec![AgentId::ClaudeCode],
            scope,
            repos: vec![RepoContext {
                name: "r".into(),
                path: PathBuf::from("/tmp/r"),
                branch: Some("main".into()),
                agent_docs: vec![AgentDoc {
                    agent: AgentId::ClaudeCode,
                    files: vec![MatchedFile {
                        path: PathBuf::from("CLAUDE.md"),
                        bytes: content.len() as u64,
                        content: content.to_string(),
                    }],
                }],
                warnings: vec![],
            }],
            warnings: vec![],
        }
    }

    #[test]
    fn context_json_renderer_writes_single_object_no_trailing_newline() {
        let ctx = synth_context(Scope::All, "body\n");
        let mut buf: Vec<u8> = Vec::new();
        render_context_json(&ctx, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.ends_with('\n'), "JSON path emits no trailing newline");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["schema_version"], 1);
    }

    #[test]
    fn context_markdown_renderer_emits_headers_and_fenced_code() {
        let ctx = synth_context(Scope::All, "hello\n");
        let mut buf: Vec<u8> = Vec::new();
        render_context_markdown(&ctx, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("# repograph context"),
            "top-level header present: {s}"
        );
        assert!(
            s.contains("## r  (branch: main)"),
            "repo header present: {s}"
        );
        assert!(s.contains("`/tmp/r`"), "path rendered as inline code: {s}");
        assert!(s.contains("### claude-code"), "agent header present: {s}");
        assert!(
            s.contains("#### CLAUDE.md ("),
            "file header with size present: {s}"
        );
        assert!(s.contains("```"), "fenced code block present: {s}");
        assert!(s.contains("hello"), "file content inlined: {s}");
    }

    #[test]
    fn context_markdown_renderer_uses_tilde_fence_when_content_contains_backtick_fence() {
        let ctx = synth_context(Scope::All, "intro\n```bash\nls\n```\nend\n");
        let mut buf: Vec<u8> = Vec::new();
        render_context_markdown(&ctx, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("~~~"), "tilde fence used: {s}");
        // The original backticks must appear inside, not be re-fenced.
        assert!(s.contains("```bash"), "embedded backticks preserved: {s}");
    }

    #[test]
    fn context_markdown_renderer_handles_workspace_scope() {
        let ctx = synth_context(
            Scope::Workspace {
                name: "team".into(),
            },
            "x",
        );
        let mut buf: Vec<u8> = Vec::new();
        render_context_markdown(&ctx, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("workspace `team`"), "scope phrase: {s}");
    }

    #[test]
    fn context_markdown_renderer_renders_warnings_as_blockquote() {
        use repograph_core::{RepoContext, SCHEMA_VERSION};
        let ctx = Context {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-05-24T00:00:00Z".into(),
            agents: vec![],
            scope: Scope::All,
            repos: vec![RepoContext {
                name: "ghost".into(),
                path: PathBuf::from("/tmp/ghost"),
                branch: None,
                agent_docs: vec![],
                warnings: vec!["path no longer accessible".into()],
            }],
            warnings: vec![],
        };
        let mut buf: Vec<u8> = Vec::new();
        render_context_markdown(&ctx, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("> **warning:** path no longer accessible"),
            "blockquote warning rendered: {s}"
        );
        assert!(
            !s.contains("###"),
            "missing repo section has no agent subheadings: {s}"
        );
    }

    #[test]
    fn human_size_handles_each_unit_band() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn pick_fence_falls_back_when_content_has_backtick_fence() {
        assert_eq!(pick_fence("plain text\n"), "```");
        assert_eq!(pick_fence("```rust\nfoo\n```"), "~~~");
        assert_eq!(pick_fence("  ```python\n"), "~~~");
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
        assert!(
            !body.contains("description"),
            "description omitted; body: {body}"
        );
    }
}
