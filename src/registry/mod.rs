use crate::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Registry not found: {0}")]
    NotFound(String),
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    #[error("Registry error: {0}")]
    RegistryError(String),
}

pub mod cli;
pub mod resolver;

pub use resolver::resolve_actor_reference;

/// Get the registry path from configuration or environment
pub fn get_registry_path() -> Option<PathBuf> {
    // Try environment variable first
    if let Ok(path) = env::var("THEATER_REGISTRY") {
        let path = PathBuf::from(path);
        if path.exists() {
            println!("Found registry path in env: {:?}", path);
            return Some(fs::canonicalize(&path).unwrap_or(path));
        }
    }

    // Try default locations
    if let Some(home) = dirs::home_dir() {
        let home_registry = home.join(".theater/registry");
        if home_registry.exists() {
            println!("Found registry in home dir: {:?}", home_registry);
            return Some(fs::canonicalize(&home_registry).unwrap_or(home_registry));
        }
    }

    // Look for local registry
    let local_registry = PathBuf::from("./registry");
    if local_registry.exists() {
        println!("Found local registry: {:?}", local_registry);
        return Some(fs::canonicalize(&local_registry).unwrap_or(local_registry));
    }

    // Project-relative registry
    let project_registry = PathBuf::from("../registry");
    if project_registry.exists() {
        println!("Found project-relative registry: {:?}", project_registry);
        return Some(fs::canonicalize(&project_registry).unwrap_or(project_registry));
    }

    // If no registry found
    None
}

/// Registry configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub name: String,
    pub description: String,
    pub version: String,
    pub component_dir: PathBuf,
    pub manifest_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub default_version_strategy: String,
    pub actor_search_paths: Vec<PathBuf>,
}

/// Actor index entry in the registry
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActorIndexEntry {
    pub name: String,
    pub versions: Vec<String>,
    pub latest: String,
    pub description: String,
}

/// Initialize a new registry
pub fn init_registry(path: &Path) -> Result<()> {
    // Get absolute path for the registry
    let registry_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // Create registry directory structure
    fs::create_dir_all(&registry_path)?;
    fs::create_dir_all(registry_path.join("components"))?;
    fs::create_dir_all(registry_path.join("manifests"))?;
    fs::create_dir_all(registry_path.join("cache"))?;

    // Create default config with absolute paths
    let config = RegistryConfig {
        name: "theater-registry".to_string(),
        description: "Actor registry for Theater runtime".to_string(),
        version: "0.1.0".to_string(),
        component_dir: registry_path.join("components"),
        manifest_dir: registry_path.join("manifests"),
        cache_dir: registry_path.join("cache"),
        default_version_strategy: "latest".to_string(),
        actor_search_paths: vec![PathBuf::from("../actors")],
    };

    // Write config.toml
    let config_str = toml::to_string(&config)?;
    fs::write(registry_path.join("config.toml"), config_str)?;

    // Create empty index
    let index = serde_json::json!({
        "last_updated": chrono::Utc::now().to_rfc3339(),
        "actors": []
    });

    let index_str = toml::to_string(&index)?;
    fs::write(registry_path.join("index.toml"), index_str)?;

    info!("Registry initialized at {:?}", registry_path);
    Ok(())
}

/// List actors in the registry
pub fn list_actors(registry_path: &Path) -> Result<Vec<ActorIndexEntry>> {
    // Get absolute path for the registry
    let registry_abs_path =
        fs::canonicalize(registry_path).unwrap_or_else(|_| registry_path.to_path_buf());

    let index_path = registry_abs_path.join("index.toml");

    if !index_path.exists() {
        return Err(
            RegistryError::NotFound(format!("Registry index not found: {:?}", index_path)).into(),
        );
    }

    let index_str = fs::read_to_string(index_path)?;
    let index: toml::Value = toml::from_str(&index_str)?;

    if let Some(actors) = index.get("actors") {
        if let Some(actors_array) = actors.as_array() {
            let entries: Result<Vec<ActorIndexEntry>> = actors_array
                .iter()
                .map(|actor| {
                    let actor_str = toml::to_string(actor)?;
                    let entry: ActorIndexEntry = toml::from_str(&actor_str)?;
                    Ok(entry)
                })
                .collect();

            return entries;
        }
    }

    Ok(vec![])
}
