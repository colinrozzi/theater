use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;

use crate::{CommandContext, error::CliError, output::formatters::ServerStarted};
use theater::logging;
use theater_server::TheaterServer;

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Address to bind the theater server to
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// logging level (simple version, e.g. 'info', 'debug')
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    /// Advanced logging filter (e.g. "theater=debug,wasmtime=info")
    /// This overrides log_level if provided
    #[arg(long)]
    pub log_filter: Option<String>,

    /// log directory
    #[arg(long, default_value = "$THEATER_HOME/logs/theater")]
    pub log_dir: String,

    /// log to stdout
    #[arg(long, default_value = "false")]
    pub log_stdout: bool,
}

/// Execute the server start command asynchronously (modernized)
pub async fn execute_async(args: &StartArgs, ctx: &CommandContext) -> Result<(), CliError> {
    // Create the runtime log file path
    let log_path = shellexpand::env(&args.log_dir)
        .map_err(|e| CliError::invalid_input("log_dir", &args.log_dir, format!("Failed to expand directory: {}", e)))?;
    let log_path = PathBuf::from(log_path.as_ref()).join("theater_server.log");

    // Determine filter string based on available args
    let filter_string = match &args.log_filter {
        Some(filter) => filter.clone(),
        None => args.log_level.clone(),
    };

    // Setup logging
    logging::setup_global_logging(log_path.clone(), &filter_string, args.log_stdout)
        .map_err(|e| CliError::invalid_input("logging", "setup", format!("Failed to setup logging: {}", e)))?;

    info!("Starting theater server on {}", args.address);
    info!("Logging to directory: {}", args.log_dir);

    // Create server info for output
    let server_info = ServerStarted {
        address: args.address,
        log_level: args.log_level.clone(),
        log_filter: args.log_filter.clone(),
        log_dir: args.log_dir.clone(),
        log_path: log_path.clone(),
        log_stdout: args.log_stdout,
        filter_string: filter_string.clone(),
    };

    // Display server start info
    ctx.output.output(&server_info, None)?;

    // Create and run the theater server
    let server = TheaterServer::new(args.address).await
        .map_err(|e| CliError::connection_failed(args.address, e))?;
    
    info!("Theater server created successfully");
    
    // Run the server (this will block until server is stopped)
    server.run().await
        .map_err(|e| CliError::connection_failed(args.address, e))?;

    Ok(())
}

/// Legacy wrapper for backward compatibility
pub fn execute(args: &StartArgs, verbose: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let config = crate::config::Config::load().unwrap_or_default();
        let output = crate::output::OutputManager::new(config.output.clone());
        let ctx = crate::CommandContext {
            config,
            output,
            verbose,
            json: false, // Server commands typically don't use JSON output
        };
        execute_async(args, &ctx).await.map_err(|e| anyhow::Error::from(e))
    })
}

/// Legacy async function for backward compatibility
pub async fn start_server(args: &StartArgs) -> Result<()> {
    let config = crate::config::Config::load().unwrap_or_default();
    let output = crate::output::OutputManager::new(config.output.clone());
    let ctx = crate::CommandContext {
        config,
        output,
        verbose: false,
        json: false,
    };
    execute_async(args, &ctx).await.map_err(|e| anyhow::Error::from(e))
}
