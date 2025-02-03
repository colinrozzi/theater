use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::EnvFilter;
use crate::theater_server::TheaterServer;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Address to bind the theater server to
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    address: SocketAddr,

    /// logging level
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Setup logging
    setup_logging(&args.log_level, true);

    info!("Starting theater server on {}", args.address);
    
    // Create and run the theater server
    let mut server = TheaterServer::new(args.address).await?;
    server.run().await?;

    Ok(())
}

fn setup_logging(level: &str, actor_only: bool) {
    let filter = if actor_only {
        EnvFilter::from_default_env()
            .add_directive(format!("theater={}", level).parse().unwrap())
            .add_directive("actix_web=info".parse().unwrap())
            .add_directive("actor=info".parse().unwrap())
            .add_directive("wasm_component=debug".parse().unwrap())
    } else {
        EnvFilter::from_default_env().add_directive(format!("theater={}", level).parse().unwrap())
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_line_number(true)
        .with_writer(std::io::stdout)
        .init();
}