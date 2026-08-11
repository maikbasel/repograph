//! Core domain library for repograph.
//!
//! Owns the `Config` model, error type, and `git2` adapters. No clap, no
//! terminal I/O, no async — the binary crate depends on this crate for both of
//! its front-ends (the CLI commands and the `repograph mcp serve` MCP server)
//! to keep domain logic separate from presentation and transport.

pub mod agent_artifact;
pub mod agents;
pub mod config;
pub mod context;
pub mod doctor;
pub mod envelope;
pub mod error;
pub mod git;
pub mod mcp_registration;
pub mod path;
pub mod search;

pub use agent_artifact::{
    ARTIFACT_BODY_VERSION, ArtifactResult, BODY as AGENT_ARTIFACT_BODY, Capability,
    DELIMITER_BEGIN, DELIMITER_BEGIN_PREFIX, DELIMITER_END, REASON_COPILOT_DEFERRED,
    SETUP_BODY as AGENT_ARTIFACT_SETUP_BODY, SETUP_SUMMARY as AGENT_ARTIFACT_SETUP_SUMMARY,
    SUMMARY as AGENT_ARTIFACT_SUMMARY, capabilities_for, has_artifact_writer, install_artifacts,
    install_one, installed_version, refresh_installed_artifacts, render_artifact, resolve_path,
    scope_is_meaningful,
};
pub use agents::AgentId;
pub use config::{
    Agents, CONFIG_FILE_NAME, Config, MAX_WORKSPACE_NAME_LEN, RESERVED_WORKSPACE_NAMES, Repo,
    RepoEdit, Settings, Workspace, WorkspaceResolution, validate_workspace_name,
};
pub use context::{
    AgentDoc, Context, MatchedFile, RepoContext, SCHEMA_VERSION, Scope, resolve_agent_docs,
};
pub use doctor::{Check, DOCTOR_SCHEMA_VERSION, DoctorReport, Finding, Severity, Summary};
pub use envelope::{FindEnvelope, ListEntry, ListEnvelope, StatusEnvelope};
pub use error::RepographError;
pub use git::{RepoState, RepoStatus, inspect, validate_git_repo};
pub use mcp_registration::{
    Host as McpHost, RegistrationResult, RegistrationStatus, SERVE_ARGS, SERVER_KEY, hosts_mcp,
    register as register_mcp, status as mcp_registration_status,
};
pub use search::{
    FIND_SCHEMA_VERSION, Hit, INDEX_DB_NAME, IndexOutcome, IndexStatus, MODEL_SUBDIR,
    RefreshOutcome, SearchOutcome, build_index, build_index_reporting, index_db_path, index_health,
    model_cache_dir, refresh_stale, search,
};

/// Crate version, sourced from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
