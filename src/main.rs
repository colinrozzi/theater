use anyhow::Result;
use clap::Parser;
use runtime_v2::{ActorRuntime, WasmActor};
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

    // Load the WASM actor from the manifest
    //println!("Loading actor from manifest: {:?}", args.manifest);
    //let actor = WasmActor::from_file(args.manifest)?;

    // Create and initialize the runtime
    println!("Creating actor runtime...");
    let mut runtime = ActorRuntime::from_file(args.manifest).await?;

    println!("Initializing actor...");
    //runtime.init().await?;

    println!("Actor initialized successfully!");
    //println!("Current chain head: {:?}", runtime.get_chain().get_head());

    // TODO: Set up HTTP server or message handler based on manifest configuration

    // For now, just keep the program running
    println!("Actor is running. Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;

    println!("Shutting down...");
    Ok(())
}
