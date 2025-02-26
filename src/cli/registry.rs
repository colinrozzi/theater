use anyhow::Result;
use clap::Subcommand;
use log::info;
use std::path::PathBuf;

use crate::registry;

#[derive(Subcommand)]
pub enum RegistryCommands {
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

pub fn handle_registry_command(command: &RegistryCommands) -> Result<()> {
    match command {
        RegistryCommands::Init { path } => {
            info!("Initializing registry at {:?}", path);
            registry::init_registry(path)
        },
        
        RegistryCommands::Update { path } => {
            info!("Updating registry at {:?}", path);
            registry::cli::cmd_update_registry(path)
        },
        
        RegistryCommands::List { path } => {
            registry::cli::cmd_list_registry_actors(path)
        },
        
        RegistryCommands::Register { path, registry } => {
            info!("Registering actor {:?} with registry {:?}", path, registry);
            registry::cli::cmd_register_actor(path, registry)
        },
    }
}
