// This is a sample CLI integration for the Theater system
// You'll need to adapt this to match your existing CLI structure

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::error::Result;
use crate::registry;
use crate::registry::resolver::resolve_actor_reference;

#[derive(Parser)]
#[command(name = "theater")]
#[command(about = "Theater runtime for WebAssembly components", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an actor
    Start {
        /// Path to actor manifest or actor name reference (e.g., "chat" or "chat:0.1.0")
        manifest_path: String,
    },
    
    /// List available actors and manifests
    List {
        #[command(subcommand)]
        command: ListCommands,
        
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },
    
    /// Registry commands
    Registry {
        #[command(subcommand)]
        command: RegistryCommands,
    },
}

#[derive(Subcommand)]
enum ListCommands {
    /// List available actors
    Actors,
    /// List available manifests
    Manifests,
}

#[derive(Subcommand)]
enum RegistryCommands {
    /// Initialize a new registry
    Init {
        /// Registry path
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    
    /// Update registry (scan for actors)
    Update {
        /// Registry path
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    
    /// List actors in registry
    List {
        /// Registry path
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    
    /// Register an actor with the registry
    Register {
        /// Path to actor manifest or directory
        path: PathBuf,
        
        /// Registry path
        #[arg(default_value = ".")]
        registry: PathBuf,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Start { manifest_path } => {
            println!("Starting actor: {}", manifest_path);
            
            // Find registry path
            let registry_path = registry::get_registry_path();
            
            // Resolve the actor reference (name or path)
            let (resolved_manifest, component_path) = 
                resolve_actor_reference(&manifest_path, registry_path.as_deref())?;
            
            // Start the actor with resolved paths
            println!("Resolved to manifest: {:?}", resolved_manifest);
            println!("Component path: {:?}", component_path);
            
            // Call your existing runtime start function with the resolved path
            crate::runtime::start_actor(&resolved_manifest.to_string_lossy())
        },
        
        Commands::List { command, verbose } => {
            match command {
                ListCommands::Actors => {
                    println!("Available actors:");
                    // Call your existing list_actors function
                    crate::runtime::list_actors(verbose)
                },
                ListCommands::Manifests => {
                    println!("Available manifests:");
                    // Call your existing list_manifests function
                    crate::runtime::list_manifests(verbose)
                }
            }
        },
        
        Commands::Registry { command } => {
            match command {
                RegistryCommands::Init { path } => {
                    println!("Initializing registry at {:?}", path);
                    registry::cli::cmd_init_registry(&path)
                },
                
                RegistryCommands::Update { path } => {
                    println!("Updating registry at {:?}", path);
                    registry::cli::cmd_update_registry(&path)
                },
                
                RegistryCommands::List { path } => {
                    registry::cli::cmd_list_registry_actors(&path)
                },
                
                RegistryCommands::Register { path, registry } => {
                    println!("Registering actor {:?} with registry {:?}", path, registry);
                    registry::cli::cmd_register_actor(&path, &registry)
                },
            }
        },
    }
}
