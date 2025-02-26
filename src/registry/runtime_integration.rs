// Sample runtime integration for the Theater system
// You'll need to adapt this to your specific runtime system

use crate::error::Result;
use super::resolver::resolve_actor_reference;
use std::path::{Path, PathBuf};
use log::{debug, info, warn};

// This function shows how to integrate the registry with your existing actor start function
pub fn start_actor_with_registry(reference: &str) -> Result<()> {
    // Get registry path
    let registry_path = super::get_registry_path();
    
    // Resolve the actor reference
    debug!("Resolving actor reference: {}", reference);
    let (manifest_path, component_path) = resolve_actor_reference(reference, registry_path.as_deref())?;
    
    info!("Starting actor from manifest: {:?}", manifest_path);
    
    // Load and parse the manifest
    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let manifest: toml::Value = toml::from_str(&manifest_content)?;
    
    // Extract necessary information from the manifest
    let name = manifest.get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("unnamed-actor");
    
    let description = manifest.get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("No description");
    
    // Print info about the actor being started
    info!("Starting actor '{}': {}", name, description);
    info!("Component path: {:?}", component_path);
    
    // Call your existing runtime's start_actor function with the resolved manifest
    // This is just a placeholder - you need to replace this with your actual implementation
    your_existing_start_actor_function(&manifest_path)
}

// This is a placeholder for your existing start_actor function
// Replace this with your actual implementation
fn your_existing_start_actor_function(manifest_path: &Path) -> Result<()> {
    // Your existing implementation here
    // This is just a placeholder that would be replaced with your actual code
    info!("Using existing start_actor function with manifest: {:?}", manifest_path);
    
    // Placeholder implementation - replace with your actual code
    Ok(())
}

// Add this to your main runtime module to expose the registry-enabled start function
pub fn start_actor(reference: &str) -> Result<()> {
    // First try to handle as a registry reference
    match start_actor_with_registry(reference) {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("Failed to start actor using registry: {}", e);
            
            // Fallback to direct path handling (your existing implementation)
            info!("Falling back to direct path handling");
            your_existing_start_actor_function(Path::new(reference))
        }
    }
}
