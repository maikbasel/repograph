//! `repograph mcp serve` — run the read-only MCP server over stdio.
//!
//! # The stdout contract inverts here
//!
//! Every other command treats stdout as data and stderr as diagnostics. Under
//! `serve`, stdout *is* the JSON-RPC frame stream: one stray byte written to it
//! corrupts the session for the client. The `tracing`-to-stderr discipline that
//! is conventional elsewhere becomes load-bearing here, which is why `run`
//! disables progress rendering before handing control to the server.

use std::path::Path;

use clap::{Parser, Subcommand};
use repograph_core::RepographError;
use rmcp::ServiceExt;
use rmcp::transport::io::stdio;

use crate::mcp::RepographServer;

#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: McpCommand,
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// Run the MCP server on stdin/stdout. Started by an MCP client (Claude
    /// Code, Cursor, Windsurf, Copilot) as a child process — not meant to be
    /// run interactively.
    Serve(ServeArgs),
}

#[derive(Debug, Parser)]
pub struct ServeArgs {}

/// Dispatch an `mcp` subcommand.
///
/// # Errors
///
/// Propagates [`RepographError`] from server startup or transport failure.
pub fn run(args: &Args, config_dir: &Path, data_dir: &Path) -> Result<(), RepographError> {
    match &args.command {
        McpCommand::Serve(_) => serve(config_dir, data_dir),
    }
}

/// Serve MCP over stdio until the client closes the connection.
///
/// Runs on a private current-thread runtime, matching how `commands::update`
/// drives axoupdater's async API. `main()` stays synchronous, and no part of
/// the CLI is forced into an async signature by this one subcommand.
#[tracing::instrument(skip_all, fields(
    config_dir = %config_dir.display(),
    data_dir = %data_dir.display(),
))]
fn serve(config_dir: &Path, data_dir: &Path) -> Result<(), RepographError> {
    tracing::debug!("mcp serve: start");

    // Config is loaded per tool call rather than here, so registry edits are
    // picked up without restarting the server. Startup therefore validates
    // nothing beyond the directories it was handed — a malformed config
    // surfaces as a tool-level error on first use, where the client can see it.
    let server = RepographServer::new(config_dir.to_path_buf(), data_dir.to_path_buf());

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RepographError::Io)?;

    runtime.block_on(async move {
        let service = server
            .serve(stdio())
            .await
            .map_err(|e| RepographError::Io(std::io::Error::other(e)))?;
        tracing::info!("mcp serve: client connected");
        service
            .waiting()
            .await
            .map_err(|e| RepographError::Io(std::io::Error::other(e)))?;
        tracing::info!("mcp serve: client disconnected");
        Ok(())
    })
}
