use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;

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

pub async fn start_server(args: &StartArgs) -> Result<()> {
    // Create the runtime log file path
    let log_path = shellexpand::env(&args.log_dir)
        .map_err(|e| anyhow::anyhow!("Failed to expand log directory: {}", e))?;
    let log_path = PathBuf::from(log_path.as_ref()).join("theater_server.log");

    // Determine filter string based on available args
    let filter_string = match &args.log_filter {
        Some(filter) => filter.clone(),
        None => args.log_level.clone(),
    };

    logging::setup_global_logging(log_path, &filter_string, args.log_stdout)
        .expect("Failed to setup logging");

    info!("Starting theater server on {}", args.address);
    info!("Logging to directory: {}", args.log_dir);

    // Create and run the theater server
    let server = TheaterServer::new(args.address).await?;
    server.run().await?;

    Ok(())
}

pub fn execute(args: &StartArgs, _verbose: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(start_server(args))?;
    Ok(())
}
