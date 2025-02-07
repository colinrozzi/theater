use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use theater::logging;
use theater::theater_server::TheaterServer;
use tracing::info;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Address to bind the theater server to
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    address: SocketAddr,

    /// logging level
    #[arg(short, long, default_value = "debug")]
    log_level: String,

    /// log directory
    #[arg(long, default_value = "logs/theater")]
    log_dir: PathBuf,

    /// log to stdout
    #[arg(long, default_value = "false")]
    log_stdout: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Create the runtime log file path
    let log_path = args.log_dir.join("theater.log");

    // Build the subscriber
    let filter_string = format!(
        //"debug,theater={},wasmtime=debug,wit_bindgen=debug",
        "{}",
        args.log_level
    );

    logging::setup_global_logging(log_path, &filter_string, args.log_stdout)
        .expect("Failed to setup logging");

    info!("Starting theater server on {}", args.address);
    info!("Logging to directory: {}", args.log_dir.display());

    // Create and run the theater server
    let server = TheaterServer::new(args.address).await?;
    server.run().await?;

    Ok(())
}
