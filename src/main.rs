use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use std::path::PathBuf;
use theater::actor_runtime::ActorRuntime;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the actor manifest file
    #[arg(short, long)]
    manifest: PathBuf,

    /// logging
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Setup logging
    setup_logging(&args.log_level, true);

    // Verify manifest file exists
    if !args.manifest.exists() {
        return Err(anyhow::anyhow!(
            "Manifest file not found: {}",
            args.manifest.display()
        ));
    }

    // Create and initialize the runtime with actor_only flag
    let runtime_components = ActorRuntime::from_file(args.manifest).await?;
    let _runtime = ActorRuntime::start(runtime_components).await?;

    // Wait for Ctrl+C
    info!("Actor started at {}", Utc::now());
    tokio::signal::ctrl_c().await?;

    info!("Shutting down...");
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
        EnvFilter::from_default_env()
            .add_directive(format!("theater={}", level).parse().unwrap())
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        .init();
}
