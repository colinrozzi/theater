use anyhow::Result;
use clap::{Parser, Subcommand};
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use theater::theater_server::{ManagementCommand, ManagementResponse};
use tracing::{info, error, debug};
use bytes::Bytes;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Address of the theater server
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    address: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a new actor
    Start {
        /// Path to the actor manifest
        manifest: PathBuf,
    },
    /// Stop an actor
    Stop {
        /// Actor ID to stop
        id: String,
    },
    /// List all running actors
    List,
    /// Subscribe to actor events
    Subscribe {
        /// Actor ID to subscribe to
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging with debug level
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .with_line_number(true)
        .with_file(true)
        .init();

    let args = Args::parse();
    
    // Connect to the server
    let stream = TcpStream::connect(&args.address).await?;
    info!("Connected to theater server at {}", args.address);
    
    let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

    // Send command based on CLI args
    let command = match args.command {
        Commands::Start { manifest } => {
            info!("Starting actor from manifest: {:?}", manifest);
            ManagementCommand::StartActor { manifest }
        },
        Commands::Stop { id } => {
            info!("Stopping actor: {}", id);
            ManagementCommand::StopActor { 
                id: id.parse()? 
            }
        },
        Commands::List => {
            info!("Listing actors");
            ManagementCommand::ListActors
        },
        Commands::Subscribe { id } => {
            info!("Subscribing to actor: {}", id);
            ManagementCommand::SubscribeToActor { 
                id: id.parse()? 
            }
        },
    };

    // Send the command
    debug!("Sending command: {:?}", command);
    let cmd_bytes = serde_json::to_vec(&command)?;
    debug!("Command serialized to {} bytes", cmd_bytes.len());
    framed.send(Bytes::from(cmd_bytes)).await?;
    debug!("Command sent, waiting for response");

    // Handle response(s)
    while let Some(msg) = framed.next().await {
        debug!("Received message from server");
        match msg {
            Ok(data) => {
                debug!("Parsing response from {} bytes", data.len());
                let response: ManagementResponse = serde_json::from_slice(&data)?;
                debug!("Parsed response: {:?}", response);
                match response {
                    ManagementResponse::ActorStarted { id } => {
                        println!("Actor started successfully with ID: {:?}", id);
                    },
                    ManagementResponse::ActorStopped { id } => {
                        println!("Actor {:?} stopped successfully", id);
                    },
                    ManagementResponse::ActorList { actors } => {
                        println!("Running actors:");
                        for actor in actors {
                            println!("  {:?}", actor);
                        }
                    },
                    ManagementResponse::Subscribed { id, subscription_id } => {
                        println!("Subscribed to actor {:?} with subscription ID: {:?}", id, subscription_id);
                        println!("Listening for events (Ctrl+C to stop)...");
                    },
                    ManagementResponse::ActorEvent { id, event } => {
                        println!("Event from actor {:?}:", id);
                        println!("  {:?}", event);
                    },
                    ManagementResponse::Unsubscribed { id } => {
                        println!("Unsubscribed from actor {:?}", id);
                    },
                    ManagementResponse::Error { message } => {
                        error!("Server error: {}", message);
                    },
                }
            },
            Err(e) => {
                error!("Error receiving response: {}", e);
                break;
            }
        }

        // If not subscribed, break after first response
        if !matches!(command, ManagementCommand::SubscribeToActor { .. }) {
            debug!("Command completed, exiting");
            break;
        }
    }

    Ok(())
}