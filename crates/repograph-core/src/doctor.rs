//! Read-only health checks against the on-disk config.
//!
//! `DoctorReport::run` walks a closed catalog of [`Check`]s against a loaded
//! [`Config`] and emits one [`Finding`] per check per target.
//!
//! Every check is read-only; no config writes, no network operations, no
//! `git fetch`. The report is the contract: a stable, versioned envelope
//! downstream consumers (CI health gates, the future MCP server) can parse
//! without ambiguity.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::Config;
use crate::context::resolve_agent_docs;
use crate::error::RepographError;
use crate::git::validate_git_repo;
use crate::search::IndexStatus;

/// Current schema version of the [`DoctorReport`] JSON envelope. Additive-only
/// at `1`; any breaking change bumps this.
pub const DOCTOR_SCHEMA_VERSION: u32 = 1;

/// Severity of a single [`Finding`]. Ordering is `Error > Warn > Ok` so the
/// sort in `DoctorReport::run` puts the most pressing findings first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The check passed for this target.
    Ok,
    /// The check surfaced a sub-optimal-but-non-broken state.
    Warn,
    /// The check surfaced a broken state that gates exit code `1`.
    Error,
}

impl Severity {
    const fn rank(self) -> u8 {
        match self {
            Self::Error => 2,
            Self::Warn => 1,
            Self::Ok => 0,
        }
    }
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Severity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

/// Closed catalog of the v1 health checks. Adding a variant is a deliberate
/// schema extension — downstream consumers branch on this enum's string value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Check {
    /// Config file exists at the resolved config dir.
    ConfigPresent,
    /// Config file parses as TOML. Only run when `ConfigPresent` passed.
    ConfigParse,
    /// `[agents]` section is present in the config.
    AgentsConfigured,
    /// `[settings].projects_root`, if set, points at an existing directory.
    ProjectsRootExists,
    /// Per repo: the registered path exists on disk.
    RepoPathExists,
    /// Per repo: the path opens as a `git2::Repository`. Only run when
    /// `RepoPathExists` passed for the same repo.
    RepoIsGitRepo,
    /// Per workspace member: the member name resolves to a registered repo.
    WorkspaceMembersResolve,
    /// Per repo × per selected agent: at least one file matches the agent's
    /// pattern set. Only run when `AgentsConfigured` passed and the
    /// selection is non-empty.
    AgentDocPresent,
    /// Per selected agent × capability: the installed skill artifact exists and
    /// its version stamp matches the running binary. Read-only — reports drift,
    /// never repairs it. Appended by the binary via
    /// [`DoctorReport::with_skill_artifact_check`].
    SkillArtifactFresh,
    /// The cross-repo search index exists and is current relative to every
    /// registered repo's HEAD. Appended by the binary via
    /// [`DoctorReport::with_index_check`].
    SearchIndex,
}

/// One row in the report.
///
/// `target` is opaque to the renderer — it's the string the human / agent
/// reads to know which repo / workspace / config path the finding is about.
/// By convention: a bare name (`"api"`, `"acme"`), a `"<repo> / <agent>"`
/// pair, or a config-file path.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub check: Check,
    pub severity: Severity,
    pub target: String,
    pub message: String,
}

/// Tally of findings by severity. `total == ok + warn + error == checks.len()`.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Summary {
    pub ok: u32,
    pub warn: u32,
    pub error: u32,
    pub total: u32,
}

/// Top-level payload emitted by `repograph doctor`. `schema_version` is the
/// contract — additive-only at `1`; breaking changes bump it.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub checks: Vec<Finding>,
    pub summary: Summary,
}

impl DoctorReport {
    /// Run every check applicable to the given config-load outcome and return
    /// a sorted [`DoctorReport`].
    ///
    /// - `config_load` is `Ok(&config)` when the config loaded (including the
    ///   "missing file → empty config" case), or `Err(&err)` when the load
    ///   itself surfaced an error (malformed TOML, I/O error other than
    ///   `NotFound`). The binary maps `PermissionDenied` to exit `4` *before*
    ///   calling this function; what reaches here is `ConfigParse` or other
    ///   non-permission `Io` failures.
    /// - `config_path` is the file the `ConfigPresent` check probes and
    ///   reports in its `target` field.
    /// - `generated_at` is the RFC 3339 UTC timestamp the binary stamps via
    ///   the `time` crate (core stays free of time deps, same pattern as
    ///   `context-command`).
    #[must_use]
    pub fn run(
        config_load: Result<&Config, &RepographError>,
        config_path: &Path,
        generated_at: String,
    ) -> Self {
        let mut findings: Vec<Finding> = Vec::new();
        let file_exists = config_path.is_file();
        findings.push(config_present_finding(config_path, file_exists));

        let config = match config_load {
            Ok(c) => {
                if file_exists {
                    findings.push(Finding {
                        check: Check::ConfigParse,
                        severity: Severity::Ok,
                        target: config_path.display().to_string(),
                        message: "config file is valid TOML".to_string(),
                    });
                }
                c
            }
            Err(err) => {
                findings.push(Finding {
                    check: Check::ConfigParse,
                    severity: Severity::Error,
                    target: config_path.display().to_string(),
                    message: format!("config could not be loaded: {err}"),
                });
                return assemble(findings, generated_at);
            }
        };

        let agents_configured = config.agents().is_some();
        findings.push(agents_configured_finding(config_path, agents_configured));
        if let Some(f) = projects_root_finding(config) {
            findings.push(f);
        }

        for (name, repo) in config.repos() {
            findings.extend(check_repo(name, &repo.path));
        }
        findings.extend(check_workspaces(config));

        if agents_configured {
            findings.extend(check_agent_docs(config));
        }

        assemble(findings, generated_at)
    }

    /// Append the search-index health finding, then re-sort and re-tally.
    ///
    /// The binary owns data-dir resolution and the [`IndexStatus`] probe, so it
    /// computes the status and folds it into the report here. The finding is
    /// `ok` when the index is present and current, `warn` when it is missing,
    /// unreadable, or stale relative to one or more repos' HEAD.
    #[must_use]
    pub fn with_index_check(mut self, status: &IndexStatus) -> Self {
        self.checks.push(index_finding(status));
        sort_findings(&mut self.checks);
        self.summary = tally(&self.checks);
        self
    }

    /// Fold in a read-only freshness check for the installed skill artifacts.
    ///
    /// For each selected agent (with a writer) and each of its capabilities,
    /// resolves the expected install path under both user and project scope,
    /// reads whichever exists, and compares its version stamp to the running
    /// binary's [`crate::agent_artifact::ARTIFACT_BODY_VERSION`]. Reports `ok`
    /// when current, `warn` when missing or stale — and never writes, creates,
    /// or repairs the artifact. When `selected` is empty (no `[agents]`), no
    /// findings are produced. The binary owns `home`/`cwd` resolution and folds
    /// the result in here, mirroring [`Self::with_index_check`].
    #[must_use]
    pub fn with_skill_artifact_check(
        mut self,
        selected: &[crate::agents::AgentId],
        home: &Path,
        cwd: &Path,
    ) -> Self {
        self.checks
            .extend(skill_artifact_findings(selected, home, cwd));
        sort_findings(&mut self.checks);
        self.summary = tally(&self.checks);
        self
    }
}

/// Judge the freshness of a single managed artifact from its contents (or
/// absence), producing an `ok`/`warn` finding. `noun` names the artifact in the
/// message ("skill artifact", "always-loaded pointer") so one rule serves both.
fn freshness_finding(target: String, noun: &str, found: Option<String>) -> Finding {
    use crate::agent_artifact::{ARTIFACT_BODY_VERSION, installed_version};

    match found {
        None => Finding {
            check: Check::SkillArtifactFresh,
            severity: Severity::Warn,
            target,
            message: format!("{noun} missing — run `repograph init`"),
        },
        Some(contents) => match installed_version(&contents) {
            Some(v) if v >= ARTIFACT_BODY_VERSION => Finding {
                check: Check::SkillArtifactFresh,
                severity: Severity::Ok,
                target,
                message: format!("{noun} current (v{v})"),
            },
            Some(v) => Finding {
                check: Check::SkillArtifactFresh,
                severity: Severity::Warn,
                target,
                message: format!(
                    "{noun} is stale (installed v{v} < current v{ARTIFACT_BODY_VERSION}) — run `repograph init`"
                ),
            },
            None => Finding {
                check: Check::SkillArtifactFresh,
                severity: Severity::Warn,
                target,
                message: format!("{noun} has no version stamp — run `repograph init`"),
            },
        },
    }
}

/// Build the per-(agent, capability) freshness findings. Pure and read-only.
fn skill_artifact_findings(
    selected: &[crate::agents::AgentId],
    home: &Path,
    cwd: &Path,
) -> Vec<Finding> {
    use crate::agent_artifact::{
        DELIMITER_BEGIN_PREFIX, Scope, capabilities_for, has_artifact_writer, resolve_path,
        resolve_pointer_path,
    };
    use crate::agents::AgentId;

    // Read the managed artifact at whichever scope has it. A file is only
    // "found" if it actually carries repograph's managed block — a shared file
    // (CLAUDE.md) may exist with user-only content, which counts as missing.
    let read_managed = |paths: [PathBuf; 2]| -> Option<String> {
        paths
            .into_iter()
            .find_map(|p| fs_err::read_to_string(&p).ok())
            .filter(|c| c.contains(DELIMITER_BEGIN_PREFIX))
    };

    let mut findings = Vec::new();
    for &agent in selected {
        if !has_artifact_writer(agent) {
            continue;
        }
        for &capability in capabilities_for(agent) {
            let target = format!("{} / {}", agent.as_str(), capability.skill_name());
            // The artifact may live at user or project scope; accept either.
            let found = read_managed([
                resolve_path(agent, capability, Scope::User, home, cwd),
                resolve_path(agent, capability, Scope::Project, home, cwd),
            ]);
            findings.push(freshness_finding(target, "skill artifact", found));
        }

        // Claude Code also gets an always-loaded CLAUDE.md pointer (user or
        // project scope). Track its freshness the same way; it shares the
        // managed-delimiter version stamp, so a version bump flags a pointer
        // that has not been re-spliced by `repograph init`.
        if agent == AgentId::ClaudeCode {
            let found = read_managed([
                resolve_pointer_path(Scope::User, home, cwd),
                resolve_pointer_path(Scope::Project, home, cwd),
            ]);
            findings.push(freshness_finding(
                format!("{} / pointer", agent.as_str()),
                "always-loaded pointer",
                found,
            ));
        }
    }
    findings
}

fn index_finding(status: &IndexStatus) -> Finding {
    const TARGET: &str = "search index";
    if !status.present {
        Finding {
            check: Check::SearchIndex,
            severity: Severity::Warn,
            target: TARGET.to_string(),
            message: "no search index built yet — run `repograph index`".to_string(),
        }
    } else if !status.readable {
        Finding {
            check: Check::SearchIndex,
            severity: Severity::Warn,
            target: TARGET.to_string(),
            message: "search index is unreadable or corrupt — run `repograph index` to rebuild"
                .to_string(),
        }
    } else if status.stale.is_empty() {
        Finding {
            check: Check::SearchIndex,
            severity: Severity::Ok,
            target: TARGET.to_string(),
            message: "search index present and current".to_string(),
        }
    } else {
        Finding {
            check: Check::SearchIndex,
            severity: Severity::Warn,
            target: status.stale.join(", "),
            message: format!(
                "search index is stale or missing for: {} — run `repograph index`",
                status.stale.join(", ")
            ),
        }
    }
}

fn config_present_finding(config_path: &Path, file_exists: bool) -> Finding {
    if file_exists {
        Finding {
            check: Check::ConfigPresent,
            severity: Severity::Ok,
            target: config_path.display().to_string(),
            message: "config file is present".to_string(),
        }
    } else {
        Finding {
            check: Check::ConfigPresent,
            severity: Severity::Error,
            target: config_path.display().to_string(),
            message: "config file does not exist".to_string(),
        }
    }
}

fn agents_configured_finding(config_path: &Path, agents_configured: bool) -> Finding {
    if agents_configured {
        Finding {
            check: Check::AgentsConfigured,
            severity: Severity::Ok,
            target: config_path.display().to_string(),
            message: "[agents] section is present".to_string(),
        }
    } else {
        Finding {
            check: Check::AgentsConfigured,
            severity: Severity::Warn,
            target: config_path.display().to_string(),
            message: "[agents] section missing — run `repograph init`".to_string(),
        }
    }
}

fn projects_root_finding(config: &Config) -> Option<Finding> {
    let root = config.settings()?.projects_root.as_deref()?;
    if root.is_dir() {
        Some(Finding {
            check: Check::ProjectsRootExists,
            severity: Severity::Ok,
            target: root.display().to_string(),
            message: "[settings].projects_root exists".to_string(),
        })
    } else {
        Some(Finding {
            check: Check::ProjectsRootExists,
            severity: Severity::Warn,
            target: root.display().to_string(),
            message: format!(
                "[settings].projects_root does not exist: {}",
                root.display()
            ),
        })
    }
}

fn check_workspaces(config: &Config) -> Vec<Finding> {
    let mut out = Vec::new();
    for (ws_name, workspace) in config.workspaces() {
        for member in &workspace.members {
            if config.repos().contains_key(member) {
                out.push(Finding {
                    check: Check::WorkspaceMembersResolve,
                    severity: Severity::Ok,
                    target: format!("{ws_name} / {member}"),
                    message: "member resolves to a registered repo".to_string(),
                });
            } else {
                out.push(Finding {
                    check: Check::WorkspaceMembersResolve,
                    severity: Severity::Warn,
                    target: ws_name.clone(),
                    message: format!(
                        "workspace member '{member}' is not a registered repo (dangling)"
                    ),
                });
            }
        }
    }
    out
}

fn check_agent_docs(config: &Config) -> Vec<Finding> {
    let mut out = Vec::new();
    let selected: &[crate::agents::AgentId] =
        config.agents().map_or(&[], |a| a.selected.as_slice());
    if selected.is_empty() {
        return out;
    }
    for (name, repo) in config.repos() {
        if !repo.path.is_dir() {
            continue;
        }
        for agent in selected {
            let (docs, _) = resolve_agent_docs(&repo.path, std::slice::from_ref(agent));
            let has_file = docs.iter().any(|d| !d.files.is_empty());
            let target = format!("{name} / {}", agent.as_str());
            if has_file {
                out.push(Finding {
                    check: Check::AgentDocPresent,
                    severity: Severity::Ok,
                    target,
                    message: "at least one matching agent doc found".to_string(),
                });
            } else {
                out.push(Finding {
                    check: Check::AgentDocPresent,
                    severity: Severity::Warn,
                    target,
                    message: format!(
                        "no files matched {} patterns ({})",
                        agent.as_str(),
                        agent.file_patterns().join(", ")
                    ),
                });
            }
        }
    }
    out
}

fn check_repo(name: &str, repo_path: &Path) -> Vec<Finding> {
    let mut out = Vec::with_capacity(2);
    if repo_path.exists() {
        out.push(Finding {
            check: Check::RepoPathExists,
            severity: Severity::Ok,
            target: name.to_string(),
            message: format!("path exists: {}", repo_path.display()),
        });
        match validate_git_repo(repo_path) {
            Ok(_) => out.push(Finding {
                check: Check::RepoIsGitRepo,
                severity: Severity::Ok,
                target: name.to_string(),
                message: "path is a git repository".to_string(),
            }),
            Err(e) => out.push(Finding {
                check: Check::RepoIsGitRepo,
                severity: Severity::Error,
                target: name.to_string(),
                message: format!("path is not a git repository: {e}"),
            }),
        }
    } else {
        out.push(Finding {
            check: Check::RepoPathExists,
            severity: Severity::Error,
            target: name.to_string(),
            message: format!("path does not exist: {}", repo_path.display()),
        });
    }
    out
}

fn assemble(mut findings: Vec<Finding>, generated_at: String) -> DoctorReport {
    sort_findings(&mut findings);
    let summary = tally(&findings);
    DoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION,
        generated_at,
        checks: findings,
        summary,
    }
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.check.cmp(&b.check))
            .then_with(|| a.target.cmp(&b.target))
    });
}

fn tally(findings: &[Finding]) -> Summary {
    findings.iter().fold(Summary::default(), |mut acc, f| {
        match f.severity {
            Severity::Ok => acc.ok += 1,
            Severity::Warn => acc.warn += 1,
            Severity::Error => acc.error += 1,
        }
        acc.total += 1;
        acc
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::agents::AgentId;
    use crate::config::{Agents, CONFIG_FILE_NAME, Repo, Settings};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn ts() -> String {
        "2026-05-24T00:00:00Z".to_string()
    }

    fn init_git_repo(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        std::fs::create_dir_all(&path).unwrap();
        let repo = git2::Repository::init(&path).unwrap();
        let sig = git2::Signature::now("T", "t@e").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        crate::path::canonicalize(&path).unwrap()
    }

    fn write_config(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(CONFIG_FILE_NAME), body).unwrap();
    }

    fn count(report: &DoctorReport, check: Check, severity: Severity) -> usize {
        report
            .checks
            .iter()
            .filter(|f| f.check == check && f.severity == severity)
            .count()
    }

    #[test]
    fn missing_config_file_emits_config_present_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let cfg = Config::default();
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::ConfigPresent, Severity::Error), 1);
        assert!(report.summary.error >= 1);
    }

    #[test]
    fn config_load_error_short_circuits_after_parse() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        // Synthesize a parse failure using a real RepographError::ConfigParse.
        write_config(tmp.path(), "[unterminated");
        let err = Config::load(tmp.path()).unwrap_err();
        let report = DoctorReport::run(Err(&err), &path, ts());
        assert_eq!(count(&report, Check::ConfigParse, Severity::Error), 1);
        // Catalog short-circuits: no per-repo checks even if config might have them.
        assert!(
            report
                .checks
                .iter()
                .all(|f| matches!(f.check, Check::ConfigPresent | Check::ConfigParse))
        );
    }

    #[test]
    fn agents_missing_emits_warn_and_skips_agent_doc_present() {
        let tmp = TempDir::new().unwrap();
        let repo = init_git_repo(tmp.path(), "api");
        let mut cfg = Config::default();
        cfg.add_repo(
            "api".into(),
            Repo {
                path: repo,
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::AgentsConfigured, Severity::Warn), 1);
        assert_eq!(count(&report, Check::AgentDocPresent, Severity::Ok), 0);
        assert_eq!(count(&report, Check::AgentDocPresent, Severity::Warn), 0);
    }

    #[test]
    fn projects_root_missing_emits_warn() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.set_settings(Some(Settings {
            projects_root: Some(tmp.path().join("does-not-exist")),
        }));
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::ProjectsRootExists, Severity::Warn), 1);
    }

    #[test]
    fn projects_root_existing_emits_ok() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.set_settings(Some(Settings {
            projects_root: Some(tmp.path().to_path_buf()),
        }));
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::ProjectsRootExists, Severity::Ok), 1);
    }

    #[test]
    fn missing_repo_path_emits_error_and_skips_git_check() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.add_repo(
            "ghost".into(),
            Repo {
                path: tmp.path().join("does-not-exist"),
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::RepoPathExists, Severity::Error), 1);
        assert_eq!(count(&report, Check::RepoIsGitRepo, Severity::Error), 0);
        assert_eq!(count(&report, Check::RepoIsGitRepo, Severity::Ok), 0);
        assert!(report.summary.error >= 1);
    }

    #[test]
    fn non_git_path_emits_repo_path_ok_and_git_error() {
        let tmp = TempDir::new().unwrap();
        let plain_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&plain_dir).unwrap();
        let mut cfg = Config::default();
        cfg.add_repo(
            "notes".into(),
            Repo {
                path: plain_dir,
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::RepoPathExists, Severity::Ok), 1);
        assert_eq!(count(&report, Check::RepoIsGitRepo, Severity::Error), 1);
    }

    #[test]
    fn healthy_git_repo_emits_both_ok() {
        let tmp = TempDir::new().unwrap();
        let repo = init_git_repo(tmp.path(), "api");
        let mut cfg = Config::default();
        cfg.add_repo(
            "api".into(),
            Repo {
                path: repo,
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::RepoPathExists, Severity::Ok), 1);
        assert_eq!(count(&report, Check::RepoIsGitRepo, Severity::Ok), 1);
    }

    #[test]
    fn dangling_workspace_member_emits_warn() {
        let tmp = TempDir::new().unwrap();
        let repo = init_git_repo(tmp.path(), "api");
        let mut cfg = Config::default();
        cfg.add_repo(
            "api".into(),
            Repo {
                path: repo,
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.create_workspace("acme".into(), None).unwrap();
        cfg.add_members("acme", &["api".into()]).unwrap();
        // Forcibly tombstone: remove `api` from registry so the workspace
        // member becomes dangling.
        cfg.remove_repo("api").unwrap();
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        let dangling = report
            .checks
            .iter()
            .filter(|f| {
                f.check == Check::WorkspaceMembersResolve
                    && f.severity == Severity::Warn
                    && f.message.contains("api")
            })
            .count();
        assert_eq!(dangling, 1);
        assert_eq!(report.summary.error, 0);
    }

    #[test]
    fn agent_doc_missing_emits_warn() {
        let tmp = TempDir::new().unwrap();
        let repo = init_git_repo(tmp.path(), "api");
        // No CLAUDE.md written.
        let mut cfg = Config::default();
        cfg.add_repo(
            "api".into(),
            Repo {
                path: repo,
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.set_agents(Some(Agents {
            selected: vec![AgentId::ClaudeCode],
        }));
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::AgentDocPresent, Severity::Warn), 1);
        assert_eq!(report.summary.error, 0);
    }

    #[test]
    fn agent_doc_present_emits_ok() {
        let tmp = TempDir::new().unwrap();
        let repo = init_git_repo(tmp.path(), "api");
        std::fs::write(repo.join("CLAUDE.md"), "context\n").unwrap();
        let mut cfg = Config::default();
        cfg.add_repo(
            "api".into(),
            Repo {
                path: repo,
                description: None,
                stack: vec![],
            },
        )
        .unwrap();
        cfg.set_agents(Some(Agents {
            selected: vec![AgentId::ClaudeCode],
        }));
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(count(&report, Check::AgentDocPresent, Severity::Ok), 1);
        assert_eq!(report.summary.error, 0);
        assert_eq!(report.summary.warn, 0);
    }

    #[test]
    fn summary_totals_match_findings() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.set_agents(Some(Agents { selected: vec![] }));
        cfg.save(tmp.path()).unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        assert_eq!(
            report.summary.total,
            report.summary.ok + report.summary.warn + report.summary.error
        );
        assert_eq!(report.summary.total as usize, report.checks.len());
    }

    #[test]
    fn findings_sorted_severity_desc_then_check_asc_then_target_asc() {
        // Synthesize a report with mixed severities and verify the sort order.
        let findings = vec![
            Finding {
                check: Check::AgentDocPresent,
                severity: Severity::Ok,
                target: "z".into(),
                message: String::new(),
            },
            Finding {
                check: Check::RepoPathExists,
                severity: Severity::Error,
                target: "a".into(),
                message: String::new(),
            },
            Finding {
                check: Check::AgentsConfigured,
                severity: Severity::Warn,
                target: "b".into(),
                message: String::new(),
            },
            Finding {
                check: Check::ConfigPresent,
                severity: Severity::Ok,
                target: "a".into(),
                message: String::new(),
            },
        ];
        let report = assemble(findings, ts());
        let order: Vec<_> = report
            .checks
            .iter()
            .map(|f| (f.severity, f.check, f.target.clone()))
            .collect();
        assert_eq!(order[0].0, Severity::Error);
        assert_eq!(order[1].0, Severity::Warn);
        assert_eq!(order[2].0, Severity::Ok);
        assert_eq!(order[3].0, Severity::Ok);
        // Ok ties: check name ascending — ConfigPresent < AgentDocPresent in enum order
        // (variants declared in spec order, not alphabetical — adjust the test if needed).
        // Re-check sort: derive `Ord` on Check sorts by variant declaration order.
        // ConfigPresent is declared first, so it comes before AgentDocPresent.
        assert!(matches!(order[2].1, Check::ConfigPresent));
        assert!(matches!(order[3].1, Check::AgentDocPresent));
    }

    #[test]
    fn severity_ordering_error_is_max() {
        assert!(Severity::Error > Severity::Warn);
        assert!(Severity::Warn > Severity::Ok);
        assert!(Severity::Error > Severity::Ok);
    }

    #[test]
    fn json_envelope_has_documented_top_level_keys() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let cfg = Config::default();
        let report = DoctorReport::run(Ok(&cfg), &path, ts());
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert!(v["generated_at"].is_string());
        assert!(v["checks"].is_array());
        assert!(v["summary"].is_object());
        assert!(v["summary"]["total"].is_number());
    }

    fn index_check(report: &DoctorReport) -> &Finding {
        report
            .checks
            .iter()
            .find(|f| f.check == Check::SearchIndex)
            .expect("index check present")
    }

    #[test]
    fn with_index_check_missing_is_warn() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_index_check(&IndexStatus::default());
        let f = index_check(&report);
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.message.contains("repograph index"));
    }

    #[test]
    fn with_index_check_present_current_is_ok() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let status = IndexStatus {
            present: true,
            readable: true,
            stale: vec![],
        };
        let report =
            DoctorReport::run(Ok(&Config::default()), &path, ts()).with_index_check(&status);
        assert_eq!(index_check(&report).severity, Severity::Ok);
    }

    #[test]
    fn with_index_check_stale_names_repo_and_warns() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let status = IndexStatus {
            present: true,
            readable: true,
            stale: vec!["api".to_string()],
        };
        let report =
            DoctorReport::run(Ok(&Config::default()), &path, ts()).with_index_check(&status);
        let f = index_check(&report);
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.message.contains("api"));
    }

    #[test]
    fn with_index_check_recomputes_summary_total() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let before = DoctorReport::run(Ok(&Config::default()), &path, ts());
        let before_total = before.summary.total;
        let after = before.with_index_check(&IndexStatus::default());
        assert_eq!(after.summary.total, before_total + 1);
        assert_eq!(after.summary.total as usize, after.checks.len());
    }

    #[test]
    fn check_serializes_as_pascal_case_variant_name() {
        let f = Finding {
            check: Check::RepoIsGitRepo,
            severity: Severity::Ok,
            target: "x".into(),
            message: "y".into(),
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["check"], "RepoIsGitRepo");
        assert_eq!(v["severity"], "ok");
    }

    // ---- skill-artifact freshness check ----

    /// Write the current-version artifact for `(agent, capability)` at its
    /// user-scope path under `home`.
    fn install_current(home: &Path, agent: AgentId, capability: crate::agent_artifact::Capability) {
        use crate::agent_artifact::{Scope, render_artifact, resolve_path};
        let path = resolve_path(
            agent,
            capability,
            Scope::User,
            home,
            Path::new("/unused-cwd"),
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, render_artifact(agent, capability)).unwrap();
    }

    #[test]
    fn skill_artifact_missing_is_warn_with_init_hint() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let home = tmp.path().join("home");
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[AgentId::ClaudeCode], &home, Path::new("/cwd"));
        // claude-code yields two capability skills + one always-loaded pointer,
        // all missing.
        assert_eq!(count(&report, Check::SkillArtifactFresh, Severity::Warn), 3);
        let f = report
            .checks
            .iter()
            .find(|f| f.check == Check::SkillArtifactFresh)
            .unwrap();
        assert!(f.message.contains("repograph init"), "missing init hint");
    }

    #[test]
    fn skill_artifact_current_is_ok() {
        use crate::agent_artifact::Capability;
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let home = tmp.path().join("home");
        install_current(&home, AgentId::ClaudeCode, Capability::Consumer);
        install_current(&home, AgentId::ClaudeCode, Capability::Setup);
        let _ = crate::agent_artifact::install_pointer(
            crate::agent_artifact::Scope::User,
            &home,
            Path::new("/cwd"),
        );
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[AgentId::ClaudeCode], &home, Path::new("/cwd"));
        // Two skills + the pointer, all current.
        assert_eq!(count(&report, Check::SkillArtifactFresh, Severity::Ok), 3);
        assert_eq!(count(&report, Check::SkillArtifactFresh, Severity::Warn), 0);
    }

    #[test]
    fn skill_artifact_stale_version_is_warn() {
        use crate::agent_artifact::{Capability, Scope, resolve_path};
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let home = tmp.path().join("home");
        // Install a deliberately old-version block for the consumer skill.
        let p = resolve_path(
            AgentId::ClaudeCode,
            Capability::Consumer,
            Scope::User,
            &home,
            Path::new("/cwd"),
        );
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            "---\nname: repograph\n---\n\n<!-- repograph:begin v0 -->\nOLD\n<!-- repograph:end -->\n",
        )
        .unwrap();
        install_current(&home, AgentId::ClaudeCode, Capability::Setup);
        // Install the pointer as current so the only stale warn is the skill.
        let _ = crate::agent_artifact::install_pointer(
            crate::agent_artifact::Scope::User,
            &home,
            Path::new("/cwd"),
        );
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[AgentId::ClaudeCode], &home, Path::new("/cwd"));
        let stale = report
            .checks
            .iter()
            .find(|f| {
                f.check == Check::SkillArtifactFresh && f.message.contains("stale")
            })
            .expect("a stale warn finding");
        assert!(stale.message.contains("repograph init"));
    }

    #[test]
    fn empty_agent_selection_produces_no_skill_findings() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[], &tmp.path().join("home"), Path::new("/cwd"));
        assert_eq!(count(&report, Check::SkillArtifactFresh, Severity::Ok), 0);
        assert_eq!(count(&report, Check::SkillArtifactFresh, Severity::Warn), 0);
    }

    #[test]
    fn skill_check_does_not_mutate_artifacts() {
        use crate::agent_artifact::Capability;
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let home = tmp.path().join("home");
        install_current(&home, AgentId::ClaudeCode, Capability::Consumer);
        install_current(&home, AgentId::ClaudeCode, Capability::Setup);
        let consumer = home.join(".claude/skills/repograph/SKILL.md");
        let before = std::fs::metadata(&consumer).unwrap().modified().unwrap();
        let _ = DoctorReport::run(Ok(&Config::default()), &path, ts()).with_skill_artifact_check(
            &[AgentId::ClaudeCode],
            &home,
            Path::new("/cwd"),
        );
        let after = std::fs::metadata(&consumer).unwrap().modified().unwrap();
        assert_eq!(before, after, "doctor must not rewrite the artifact");
    }

    #[test]
    fn always_loaded_pointer_freshness_is_tracked() {
        use crate::agent_artifact::{Scope, install_pointer};
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cwd");

        // Missing pointer → a warn naming the pointer and the init hint.
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[AgentId::ClaudeCode], &home, &cwd);
        let missing = report
            .checks
            .iter()
            .find(|f| f.check == Check::SkillArtifactFresh && f.target.contains("pointer"))
            .expect("a pointer finding");
        assert_eq!(missing.severity, Severity::Warn);
        assert!(missing.message.contains("missing"), "names the gap");
        assert!(missing.message.contains("repograph init"));

        // Install the pointer (project scope) → the finding flips to ok.
        std::fs::create_dir_all(&cwd).unwrap();
        let _ = install_pointer(Scope::Project, &home, &cwd);
        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[AgentId::ClaudeCode], &home, &cwd);
        let ok = report
            .checks
            .iter()
            .find(|f| f.check == Check::SkillArtifactFresh && f.target.contains("pointer"))
            .expect("a pointer finding");
        assert_eq!(ok.severity, Severity::Ok, "installed pointer reads current");
    }

    #[test]
    fn user_authored_claude_md_without_block_reads_as_missing_pointer() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(CONFIG_FILE_NAME);
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cwd");
        std::fs::create_dir_all(&cwd).unwrap();
        // A CLAUDE.md with only user prose (no managed block) must not be
        // mistaken for an installed-but-unstamped pointer.
        std::fs::write(cwd.join("CLAUDE.md"), "# My rules\n\nNo repograph here.\n").unwrap();

        let report = DoctorReport::run(Ok(&Config::default()), &path, ts())
            .with_skill_artifact_check(&[AgentId::ClaudeCode], &home, &cwd);
        let pointer = report
            .checks
            .iter()
            .find(|f| f.check == Check::SkillArtifactFresh && f.target.contains("pointer"))
            .expect("a pointer finding");
        assert!(
            pointer.message.contains("missing"),
            "unblocked CLAUDE.md is missing, not unstamped, got: {}",
            pointer.message
        );
    }
}
