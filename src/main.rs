use anyhow::Result;
use clap::Parser;
use runtime_v2::ActorRuntime;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the actor manifest file
    #[arg(short, long)]
    manifest: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Create and initialize the runtime
    println!("Creating actor runtime...");
    let mut runtime = ActorRuntime::from_file(args.manifest).await?;

    println!("Actor '{}' initialized successfully!", runtime.config.name);

    // Wait for Ctrl+C
    println!("Actor is running. Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;

    println!("Shutting down...");
    runtime.shutdown().await?;

    Ok(())
}
