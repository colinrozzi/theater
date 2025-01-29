use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use theater::messages::TheaterCommand;
use theater::theater_runtime::TheaterRuntime;
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

    let mut theater = TheaterRuntime::new().await?;

    let theater_tx = theater.theater_tx.clone();

    // Start the theater runtime
    let theater_handle = tokio::spawn(async move {
        theater.run().await.unwrap();
    });

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let _ = theater_tx
        .send(TheaterCommand::SpawnActor {
            manifest_path: args.manifest.clone(),
            response_tx,
            parent_id: None,
        })
        .await;

    let actor_id = response_rx.await?;
    info!("Actor spawned with id: {:?}", actor_id?);

    // Wait for the theater runtime to finish
    theater_handle.await?;

    // Wait for ctrl-c
    tokio::signal::ctrl_c().await?;

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
