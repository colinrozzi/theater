use crate::registry::{RegistryManager, ResourceType};
use crate::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct RegistryUriArgs {
    #[command(subcommand)]
    pub command: RegistryUriCommand,
}

#[derive(Subcommand)]
pub enum RegistryUriCommand {
    /// Create a registry configuration file
    CreateConfig {
        /// Path where to create the registry configuration
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    
    /// List resources in the registry
    List {
        /// Registry configuration file path
        #[arg(short, long)]
        config: PathBuf,
        
        /// Optional registry name (uses default if not specified)
        #[arg(short, long)]
        registry: Option<String>,
        
        /// Resource type (components, manifests, states)
        #[arg(short, long)]
        resource_type: Option<String>,
        
        /// Optional category filter
        #[arg(short, long)]
        category: Option<String>,
    },
    
    /// Publish a component to the registry
    PublishComponent {
        /// Registry configuration file path
        #[arg(short, long)]
        config: PathBuf,
        
        /// Path to the component file
        #[arg(short, long)]
        component: PathBuf,
        
        /// Category for the component
        #[arg(short, long)]
        category: String,
        
        /// Version for the component
        #[arg(short, long)]
        version: String,
        
        /// Optional name for the component (defaults to filename)
        #[arg(short, long)]
        name: Option<String>,
        
        /// Optional registry name (uses default if not specified)
        #[arg(short, long)]
        registry: Option<String>,
    },
    
    /// Publish a manifest to the registry
    PublishManifest {
        /// Registry configuration file path
        #[arg(short, long)]
        config: PathBuf,
        
        /// Path to the manifest file
        #[arg(short, long)]
        manifest: PathBuf,
        
        /// Category for the manifest
        #[arg(short, long)]
        category: String,
        
        /// Optional version for the manifest
        #[arg(short, long)]
        version: Option<String>,
        
        /// Optional name for the manifest (defaults to filename)
        #[arg(short, long)]
        name: Option<String>,
        
        /// Optional registry name (uses default if not specified)
        #[arg(short, long)]
        registry: Option<String>,
    },
}

/// Handle registry URI commands
pub fn handle_registry_uri_command(args: &RegistryUriArgs) -> Result<()> {
    match &args.command {
        RegistryUriCommand::CreateConfig { path } => {
            crate::registry::cli::cmd_create_registry_config(path)?;
            println!("Registry configuration created successfully");
        }
        
        RegistryUriCommand::List {
            config,
            registry,
            resource_type,
            category,
        } => {
            let registry_manager = crate::registry::cli::cmd_init_registry_from_config(config)?;
            crate::registry::cli::cmd_list_registry_resources(
                &registry_manager,
                registry.as_deref(),
                resource_type.as_deref(),
                category.as_deref(),
            )?;
        }
        
        RegistryUriCommand::PublishComponent {
            config,
            component,
            category,
            version,
            name,
            registry,
        } => {
            let registry_manager = crate::registry::cli::cmd_init_registry_from_config(config)?;
            crate::registry::cli::cmd_publish_component(
                &registry_manager,
                component,
                category,
                version,
                name.as_deref(),
                registry.as_deref(),
            )?;
        }
        
        RegistryUriCommand::PublishManifest {
            config,
            manifest,
            category,
            version,
            name,
            registry,
        } => {
            let registry_manager = crate::registry::cli::cmd_init_registry_from_config(config)?;
            crate::registry::cli::cmd_publish_manifest(
                &registry_manager,
                manifest,
                category,
                version.as_deref(),
                name.as_deref(),
                registry.as_deref(),
            )?;
        }
    }
    
    Ok(())
}
