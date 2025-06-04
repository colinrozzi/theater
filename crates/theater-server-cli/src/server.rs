use anyhow::Result;
use std::path::PathBuf;
use theater::logging;
use theater_server::TheaterServer;
use tracing::info;

use crate::args::ServerArgs;

pub async fn start_server(args: &ServerArgs) -> Result<()> {
    // Create the runtime log file path
    let log_path = shellexpand::env(&args.log_dir)
        .map_err(|e| anyhow::anyhow!("Failed to expand log directory: {}", e))?;
    let log_path = PathBuf::from(log_path.as_ref()).join("theater_server.log");

    let log_level = args.log_level.parse().unwrap_or_else(|_| {
        eprintln!(
            "Invalid log level: {}. Defaulting to 'info'.",
            args.log_level
        );
        tracing::Level::INFO
    });

    logging::setup_global_logging(log_path, &log_level, args.log_stdout)
        .expect("Failed to setup logging");

    info!("Starting theater server on {}", args.address);
    info!("Logging to directory: {}", args.log_dir);

    // Create and run the theater server
    let server = TheaterServer::new(args.address).await?;
    server.run().await?;

    Ok(())
}
