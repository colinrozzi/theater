use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;

use crate::logging;
use crate::theater_server::TheaterServer;

#[derive(Debug, Parser)]
pub struct StartArgs {
    /// Address to bind the theater server to
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    pub address: SocketAddr,

    /// logging level
    #[arg(short, long, default_value = "info")]
    pub log_level: String,

    /// log directory
    #[arg(long, default_value = "logs/theater")]
    pub log_dir: PathBuf,

    /// log to stdout
    #[arg(long, default_value = "false")]
    pub log_stdout: bool,
}

pub async fn start_server(args: &StartArgs) -> Result<()> {
    // Create the runtime log file path
    let log_path = args.log_dir.join("theater.log");

    // Build the subscriber
    let filter_string = format!("{}", args.log_level);

    logging::setup_global_logging(log_path, &filter_string, args.log_stdout)
        .expect("Failed to setup logging");

    info!("Starting theater server on {}", args.address);
    info!("Logging to directory: {}", args.log_dir.display());

    // Create and run the theater server
    let server = TheaterServer::new(args.address).await?;
    server.run().await?;

    Ok(())
}

pub fn execute(args: &StartArgs, verbose: bool) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(start_server(args))?;
    Ok(())
}
