use crate::error::Result;
use super::{init_registry, list_actors, get_registry_path};
use log::{debug, info, warn};
use std::path::{Path, PathBuf};
use std::fs;
use std::env;
use walkdir::WalkDir;

/// Initialize a new registry
pub fn cmd_init_registry(path: &Path) -> Result<()> {
    init_registry(path)
}

/// Update the registry by scanning actor search paths
pub fn cmd_update_registry(path: &Path) -> Result<()> {
    // Load config
    let config_path = path.join("config.toml");
    let config_str = fs::read_to_string(&config_path)?;
    let config: toml::Value = toml::from_str(&config_str)?;
    
    // Get search paths
    let search_paths = if let Some(paths) = config.get("actor_search_paths") {
        if let Some(paths_array) = paths.as_array() {
            paths_array.iter()
                .filter_map(|p| p.as_str())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        } else {
            return Err(crate::error::Error::InvalidFormat("actor_search_paths is not an array".to_string()));
        }
    } else {
        return Err(crate::error::Error::InvalidFormat("actor_search_paths not found in config".to_string()));
    };
    
    // Get component and manifest directories
    let component_dir = if let Some(dir) = config.get("component_dir") {
        PathBuf::from(dir.as_str().unwrap_or("./components"))
    } else {
        path.join("components")
    };
    
    let manifest_dir = if let Some(dir) = config.get("manifest_dir") {
        PathBuf::from(dir.as_str().unwrap_or("./manifests"))
    } else {
        path.join("manifests")
    };
    
    // Create index structure
    let mut actors_index = Vec::new();
    let mut processed_actors = std::collections::HashMap::new();
    
    // Load existing index if available
    let index_path = path.join("index.toml");
    if index_path.exists() {
        let index_str = fs::read_to_string(&index_path)?;
        let index: toml::Value = toml::from_str(&index_str)?;
        
        if let Some(actors) = index.get("actors") {
            if let Some(actors_array) = actors.as_array() {
                for actor in actors_array {
                    if let (Some(name), Some(latest), Some(versions), Some(desc)) = (
                        actor.get("name").and_then(|n| n.as_str()),
                        actor.get("latest").and_then(|l| l.as_str()),
                        actor.get("versions").and_then(|v| v.as_array()),
                        actor.get("description").and_then(|d| d.as_str())
                    ) {
                        let versions: Vec<String> = versions.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        
                        processed_actors.insert(name.to_string(), (latest.to_string(), versions, desc.to_string()));
                    }
                }
            }
        }
    }
    
    // Scan search paths for actor.toml files
    for search_path in search_paths {
        debug!("Scanning for actors in {:?}", search_path);
        
        for entry in WalkDir::new(&search_path)
            .max_depth(3)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_name() == "actor.toml")
        {
            let manifest_path = entry.path();
            debug!("Found actor manifest: {:?}", manifest_path);
            
            // Read the manifest
            match fs::read_to_string(manifest_path) {
                Ok(manifest_str) => {
                    match toml::from_str::<toml::Value>(&manifest_str) {
                        Ok(manifest) => {
                            if let (Some(name), Some(version), Some(description), Some(component_path)) = (
                                manifest.get("name").and_then(|n| n.as_str()),
                                manifest.get("version").and_then(|v| v.as_str()),
                                manifest.get("description").and_then(|d| d.as_str()),
                                manifest.get("component_path").and_then(|c| c.as_str())
                            ) {
                                // Process the actor
                                process_actor(
                                    name, 
                                    version,
                                    description,
                                    component_path,
                                    manifest_path,
                                    &component_dir,
                                    &manifest_dir,
                                    &mut processed_actors
                                )?;
                            } else {
                                warn!("Missing required fields in manifest: {:?}", manifest_path);
                            }
                        },
                        Err(e) => {
                            warn!("Failed to parse manifest {:?}: {}", manifest_path, e);
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to read manifest {:?}: {}", manifest_path, e);
                }
            }
        }
    }
    
    // Convert processed_actors to index format
    for (name, (latest, versions, description)) in processed_actors {
        actors_index.push(serde_json::json!({
            "name": name,
            "latest": latest,
            "versions": versions,
            "description": description
        }));
    }
    
    // Write updated index
    let updated_index = serde_json::json!({
        "last_updated": chrono::Utc::now().to_rfc3339(),
        "actors": actors_index
    });
    
    let index_str = toml::to_string(&updated_index)?;
    fs::write(index_path, index_str)?;
    
    info!("Registry updated successfully");
    Ok(())
}

/// Process an actor and add it to the registry
fn process_actor(
    name: &str,
    version: &str,
    description: &str,
    component_path: &str,
    manifest_path: &Path,
    component_dir: &Path,
    manifest_dir: &Path,
    processed_actors: &mut std::collections::HashMap<String, (String, Vec<String>, String)>
) -> Result<()> {
    // Create target directories
    let target_component_dir = component_dir.join(name).join(version);
    let target_manifest_dir = manifest_dir.join(name).join(version);
    
    fs::create_dir_all(&target_component_dir)?;
    fs::create_dir_all(&target_manifest_dir)?;
    
    // Copy the component file
    let src_component_path = if Path::new(component_path).is_absolute() {
        PathBuf::from(component_path)
    } else {
        manifest_path.parent()
            .unwrap_or(Path::new("."))
            .join(component_path)
    };
    
    let target_component_path = target_component_dir.join(format!("{}.wasm", name));
    
    if src_component_path.exists() {
        fs::copy(&src_component_path, &target_component_path)?;
        debug!("Copied component from {:?} to {:?}", src_component_path, target_component_path);
    } else {
        warn!("Component not found: {:?}", src_component_path);
        return Err(crate::error::Error::NotFound(format!("Component not found: {:?}", src_component_path)));
    }
    
    // Create modified manifest with relative paths
    let manifest_str = fs::read_to_string(manifest_path)?;
    let mut manifest: toml::Value = toml::from_str(&manifest_str)?;
    
    // Update component_path to be relative
    if let Some(component) = manifest.get_mut("component_path") {
        *component = toml::Value::String(format!("{}.wasm", name));
    }
    
    // Update init_state if present
    if let Some(init_state) = manifest.get("init_state") {
        if let Some(init_state_str) = init_state.as_str() {
            let src_init_path = if Path::new(init_state_str).is_absolute() {
                PathBuf::from(init_state_str)
            } else {
                manifest_path.parent()
                    .unwrap_or(Path::new("."))
                    .join(init_state_str)
            };
            
            let target_init_path = target_manifest_dir.join(Path::new(init_state_str).file_name().unwrap_or_default());
            
            if src_init_path.exists() {
                fs::copy(&src_init_path, &target_init_path)?;
                debug!("Copied init state from {:?} to {:?}", src_init_path, target_init_path);
                
                // Update init_state path in manifest
                if let Some(init_state) = manifest.get_mut("init_state") {
                    *init_state = toml::Value::String(target_init_path.file_name().unwrap_or_default().to_string_lossy().to_string());
                }
            }
        }
    }
    
    // Write modified manifest
    let modified_manifest_str = toml::to_string(&manifest)?;
    fs::write(target_manifest_dir.join("actor.toml"), modified_manifest_str)?;
    
    // Update the processed_actors map
    let entry = processed_actors.entry(name.to_string()).or_insert_with(|| (
        version.to_string(),
        Vec::new(),
        description.to_string()
    ));
    
    // Add version if not already present
    if !entry.1.contains(&version.to_string()) {
        entry.1.push(version.to_string());
    }
    
    // Update latest version if newer
    if semver::Version::parse(version).ok() > semver::Version::parse(&entry.0).ok() {
        entry.0 = version.to_string();
    }
    
    Ok(())
}

/// Display actors in the registry
pub fn cmd_list_registry_actors(path: &Path) -> Result<()> {
    let actors = list_actors(path)?;
    
    println!("Actors in registry:");
    if actors.is_empty() {
        println!("  No actors found");
    } else {
        for actor in actors {
            println!("  {} [latest: {}]", actor.name, actor.latest);
            println!("    {}", actor.description);
            println!("    Versions: {}", actor.versions.join(", "));
        }
    }
    
    Ok(())
}

/// Register an actor with the registry
pub fn cmd_register_actor(actor_path: &Path, registry_path: &Path) -> Result<()> {
    // Determine if path is to a manifest or a directory
    let manifest_path = if actor_path.is_dir() {
        actor_path.join("actor.toml")
    } else {
        actor_path.to_path_buf()
    };
    
    if !manifest_path.exists() {
        return Err(crate::error::Error::NotFound(format!("Actor manifest not found: {:?}", manifest_path)));
    }
    
    // Read the manifest
    let manifest_str = fs::read_to_string(&manifest_path)?;
    let manifest: toml::Value = toml::from_str(&manifest_str)?;
    
    // Extract required fields
    let name = manifest.get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| crate::error::Error::InvalidFormat("name not found in manifest".to_string()))?;
    
    let version = manifest.get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::error::Error::InvalidFormat("version not found in manifest".to_string()))?;
    
    let description = manifest.get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("No description");
    
    let component_path = manifest.get("component_path")
        .and_then(|c| c.as_str())
        .ok_or_else(|| crate::error::Error::InvalidFormat("component_path not found in manifest".to_string()))?;
    
    // Load config
    let config_path = registry_path.join("config.toml");
    let config_str = fs::read_to_string(&config_path)?;
    let config: toml::Value = toml::from_str(&config_str)?;
    
    // Get component and manifest directories
    let component_dir = if let Some(dir) = config.get("component_dir") {
        PathBuf::from(dir.as_str().unwrap_or("./components"))
    } else {
        registry_path.join("components")
    };
    
    let manifest_dir = if let Some(dir) = config.get("manifest_dir") {
        PathBuf::from(dir.as_str().unwrap_or("./manifests"))
    } else {
        registry_path.join("manifests")
    };
    
    // Load existing index
    let index_path = registry_path.join("index.toml");
    let mut processed_actors = std::collections::HashMap::new();
    
    if index_path.exists() {
        let index_str = fs::read_to_string(&index_path)?;
        let index: toml::Value = toml::from_str(&index_str)?;
        
        if let Some(actors) = index.get("actors") {
            if let Some(actors_array) = actors.as_array() {
                for actor in actors_array {
                    if let (Some(name), Some(latest), Some(versions), Some(desc)) = (
                        actor.get("name").and_then(|n| n.as_str()),
                        actor.get("latest").and_then(|l| l.as_str()),
                        actor.get("versions").and_then(|v| v.as_array()),
                        actor.get("description").and_then(|d| d.as_str())
                    ) {
                        let versions: Vec<String> = versions.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        
                        processed_actors.insert(name.to_string(), (latest.to_string(), versions, desc.to_string()));
                    }
                }
            }
        }
    }
    
    // Process the actor
    process_actor(
        name,
        version,
        description,
        component_path,
        &manifest_path,
        &component_dir,
        &manifest_dir,
        &mut processed_actors
    )?;
    
    // Convert processed_actors to index format
    let mut actors_index = Vec::new();
    for (name, (latest, versions, description)) in processed_actors {
        actors_index.push(serde_json::json!({
            "name": name,
            "latest": latest,
            "versions": versions,
            "description": description
        }));
    }
    
    // Write updated index
    let updated_index = serde_json::json!({
        "last_updated": chrono::Utc::now().to_rfc3339(),
        "actors": actors_index
    });
    
    let index_str = toml::to_string(&updated_index)?;
    fs::write(index_path, index_str)?;
    
    info!("Actor registered successfully");
    Ok(())
}
