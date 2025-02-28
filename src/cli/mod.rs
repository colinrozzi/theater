// Theater CLI module
pub mod actor;
pub mod manifest;
pub mod system;
pub mod dev;
pub mod legacy;
pub mod store;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "theater",
    about = "WebAssembly actor system management CLI",
    author, 
    version, 
    long_about = None
)]
pub struct Args {
    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start a new actor (legacy command)
    Start {
        /// Path to the actor manifest
        #[arg(value_name = "MANIFEST")]
        manifest: Option<String>,
        
        /// Address of the theater server
        #[arg(short, long, default_value = "127.0.0.1:9000")]
        address: String,
    },
    /// Stop an actor (legacy command)
    Stop {
        /// Actor ID to stop
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,
        
        /// Address of the theater server
        #[arg(short, long, default_value = "127.0.0.1:9000")]
        address: String,
    },
    /// List all running actors (legacy command)
    List {
        /// Show detailed information about each actor
        #[arg(short, long)]
        detailed: bool,
        
        /// Address of the theater server
        #[arg(short, long, default_value = "127.0.0.1:9000")]
        address: String,
    },
    /// Subscribe to actor events (legacy command)
    Subscribe {
        /// Actor ID to subscribe to
        #[arg(value_name = "ACTOR_ID")]
        id: Option<String>,
        
        /// Address of the theater server
        #[arg(short, long, default_value = "127.0.0.1:9000")]
        address: String,
    },
    /// Interactive mode (legacy command)
    #[command(alias = "i")]
    Interactive {
        /// Address of the theater server
        #[arg(short, long, default_value = "127.0.0.1:9000")]
        address: String,
    },
    
    /// Manage actor manifests
    #[command(subcommand)]
    Manifest(manifest::ManifestCommands),
    
    /// Manage actors
    #[command(subcommand)]
    Actor(actor::ActorCommands),
    
    /// Manage theater system
    #[command(subcommand)]
    System(system::SystemCommands),
    
    /// Development utilities
    #[command(subcommand)]
    Dev(dev::DevCommands),

    /// Content-addressed store operations
    #[command(subcommand)]
    Store(store::StoreCommands),
}
