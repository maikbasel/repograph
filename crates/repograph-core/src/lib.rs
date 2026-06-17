//! Core domain library for repograph.
//!
//! Owns the `Config` model, error type, and `git2` adapters. No clap, no
//! terminal I/O — the binary crate (and a future MCP server) depend on this
//! crate to keep their concerns separate.

pub mod agent_artifact;
pub mod agents;
pub mod config;
pub mod context;
pub mod doctor;
pub mod error;
pub mod git;
pub mod path;
pub mod search;

pub use agent_artifact::{
    ARTIFACT_BODY_VERSION, ArtifactResult, BODY as AGENT_ARTIFACT_BODY, Capability, DELIMITER_BEGIN,
    DELIMITER_BEGIN_PREFIX, DELIMITER_END, REASON_COPILOT_DEFERRED,
    SETUP_BODY as AGENT_ARTIFACT_SETUP_BODY, SETUP_SUMMARY as AGENT_ARTIFACT_SETUP_SUMMARY,
    SUMMARY as AGENT_ARTIFACT_SUMMARY, capabilities_for, has_artifact_writer, install_artifacts,
    install_one, installed_version, render_artifact, resolve_path, scope_is_meaningful,
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
pub use error::RepographError;
pub use git::{RepoState, RepoStatus, inspect, validate_git_repo};
pub use search::{
    FIND_SCHEMA_VERSION, Hit, INDEX_DB_NAME, IndexOutcome, IndexStatus, MODEL_SUBDIR,
    SearchOutcome, build_index, index_db_path, index_health, model_cache_dir, search,
};

/// Crate version, sourced from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
