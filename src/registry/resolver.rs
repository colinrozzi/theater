use crate::registry::RegistryError;
use crate::Result;
use log::{debug, warn};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;

/// Resolve an actor reference to manifest and component paths
///
/// Handles:
/// - Direct paths: "/path/to/actor.toml"
/// - Registry names: "chat" (latest version)
/// - Versioned names: "chat:0.1.0" (specific version)
pub fn resolve_actor_reference(
    reference: &str,
    registry_path: Option<&Path>,
) -> Result<(PathBuf, PathBuf), RegistryError> {
    // Check if reference is a direct path
    let reference_path = Path::new(reference);
    if reference_path.exists() && reference_path.is_file() {
        debug!("Using direct actor reference: {}", reference);
        return resolve_direct_actor_path(reference_path);
    }

    // Try to resolve using registry
    if let Some(reg_path) = registry_path {
        if let Ok((manifest_path, component_path)) = resolve_registry_actor(reference, reg_path) {
            return Ok((manifest_path, component_path));
        }
    }

    // Handle as a direct path (which might fail if it doesn't exist)
    warn!("Could not resolve actor reference through registry, treating as direct path");
    resolve_direct_actor_path(reference_path)
}

/// Resolve a direct path to an actor manifest
fn resolve_direct_actor_path(path: &Path) -> Result<(PathBuf, PathBuf), RegistryError> {
    if !path.exists() {
        return Err(
            RegistryError::NotFound(format!("Actor manifest not found: {:?}", path)).into(),
        );
    }

    // Read manifest to extract component path
    let manifest_str = fs::read_to_string(path)
        .map_err(|_| RegistryError::NotFound(format!("Actor manifest not found: {:?}", path)))?;
    let mut manifest: Value = toml::from_str(&manifest_str)
        .map_err(|e| RegistryError::InvalidFormat(format!("Invalid actor manifest: {:?}", e)))?;

    // Extract component path and convert to absolute if needed
    let component_path = if let Some(component) = manifest.get("component_path") {
        if let Some(component_str) = component.as_str() {
            let component_path = PathBuf::from(component_str);

            // If component path is relative, resolve it relative to manifest directory
            if !component_path.is_absolute() {
                if let Some(parent) = path.parent() {
                    parent.join(&component_path)
                } else {
                    component_path
                }
            } else {
                component_path
            }
        } else {
            return Err(
                RegistryError::InvalidFormat("component_path is not a string".to_string()).into(),
            );
        }
    } else {
        return Err(RegistryError::InvalidFormat(
            "component_path not found in manifest".to_string(),
        )
        .into());
    };

    // Create a temporary manifest with absolute component path
    debug!("Original component path: {:?}", component_path);
    if let Some(component_val) = manifest.get_mut("component_path") {
        *component_val = toml::Value::String(component_path.to_string_lossy().to_string());
        debug!(
            "Updated component path to absolute: {}",
            component_path.to_string_lossy()
        );
    }

    // Also update init_state to absolute path if it exists
    if let Some(init_state_val) = manifest.get_mut("init_state") {
        if let Some(init_state_str) = init_state_val.as_str() {
            let init_state_path = PathBuf::from(init_state_str);
            if !init_state_path.is_absolute() {
                if let Some(parent) = path.parent() {
                    let abs_init_path = parent.join(&init_state_path);
                    debug!(
                        "Updating init_state from {} to {}",
                        init_state_str,
                        abs_init_path.display()
                    );
                    *init_state_val =
                        toml::Value::String(abs_init_path.to_string_lossy().to_string());
                }
            }
        }
    }

    // Create a temporary directory for the modified manifest
    let temp_dir = std::env::temp_dir().join("theater_registry");
    fs::create_dir_all(&temp_dir).map_err(|e| {
        RegistryError::RegistryError(format!("Failed to create temp directory: {}", e)).into()
    })?;

    // Write the modified manifest to a temporary file
    let actor_name = manifest
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("unknown");

    let actor_version = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0");

    let temp_manifest_path = temp_dir.join(format!("{}-{}-direct.toml", actor_name, actor_version));
    let modified_manifest_str = toml::to_string(&manifest).map_err(|e| {
        RegistryError::RegistryError(format!("Failed to serialize manifest: {}", e)).into()
    })?;

    fs::write(&temp_manifest_path, modified_manifest_str).map_err(|e| {
        RegistryError::RegistryError(format!("Failed to write temporary manifest: {}", e)).into()
    })?;

    debug!(
        "Created temporary manifest with absolute paths at {:?}",
        temp_manifest_path
    );

    Ok((temp_manifest_path, component_path))
}

/// Parse an actor reference into name and version
///
/// Examples:
/// - "chat" -> ("chat", None)
/// - "chat:0.1.0" -> ("chat", Some("0.1.0"))
fn parse_actor_reference(reference: &str) -> (String, Option<String>) {
    if let Some((name, version)) = reference.split_once(':') {
        (name.to_string(), Some(version.to_string()))
    } else {
        (reference.to_string(), None)
    }
}

/// Resolve an actor using the registry
fn resolve_registry_actor(
    reference: &str,
    registry_path: &Path,
) -> Result<(PathBuf, PathBuf), RegistryError> {
    // Parse the reference
    let (name, version) = parse_actor_reference(reference);

    // Get paths to components and manifests
    let config_path = registry_path.join("config.toml");
    let config_str = fs::read_to_string(&config_path).map_err(|_| {
        RegistryError::NotFound(format!("Registry config not found: {:?}", config_path)).into()
    })?;

    let config: Value = toml::from_str(&config_str).map_err(|e| {
        RegistryError::InvalidFormat(format!("Invalid registry config: {:?}", e)).into()
    })?;

    // Extract directories from config
    let manifest_dir = extract_path_from_config(&config, "manifest_dir").map_err(|e| {
        warn!("Error extracting manifest_dir from config: {:?}", e);
        RegistryError::InvalidFormat("Invalid registry config".to_string()).into()
    })?;
    let component_dir = extract_path_from_config(&config, "component_dir").map_err(|e| {
        warn!("Error extracting component_dir from config: {:?}", e);
        RegistryError::InvalidFormat("Invalid registry config".to_string()).into()
    })?;

    // Load the index to get version info
    let index_path = registry_path.join("index.toml");
    let index_str = fs::read_to_string(&index_path).map_err(|_| {
        RegistryError::NotFound(format!("Registry index not found: {:?}", index_path)).into()
    })?;

    let index: Value = toml::from_str(&index_str).map_err(|e| {
        RegistryError::InvalidFormat(format!("Invalid registry index: {:?}", e)).into()
    })?;

    // Find the actor in the index
    let actors = index
        .get("actors")
        .and_then(|a| a.as_array())
        .ok_or_else(|| {
            RegistryError::InvalidFormat("actors list not found in registry index".to_string())
                .into()
        })?;

    let actor = actors
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(&name))
        .ok_or_else(|| {
            RegistryError::NotFound(format!("Actor '{}' not found in registry", name)).into()
        })?;

    // Determine which version to use
    let version_to_use = match version {
        Some(v) => {
            // Verify this version exists
            let versions = actor
                .get("versions")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    RegistryError::InvalidFormat("versions list not found for actor".to_string())
                        .into()
                })?;

            let version_exists = versions.iter().any(|ver| ver.as_str() == Some(&v));

            if !version_exists {
                return Err(RegistryError::NotFound(format!(
                    "Version '{}' not found for actor '{}'",
                    v, name
                ))
                .into());
            }

            v
        }
        None => {
            // Use latest version
            actor
                .get("latest")
                .and_then(|l| l.as_str())
                .ok_or_else(|| {
                    RegistryError::InvalidFormat("latest version not found for actor".to_string())
                        .into()
                })?
                .to_string()
        }
    };

    // Construct paths to manifest and component
    let manifest_rel_path = PathBuf::from(manifest_dir)
        .join(&name)
        .join(&version_to_use)
        .join("actor.toml");

    let component_rel_path = PathBuf::from(component_dir)
        .join(&name)
        .join(&version_to_use)
        .join(format!("{}.wasm", name));

    // Convert to absolute paths
    let manifest_path = if manifest_rel_path.is_absolute() {
        manifest_rel_path
    } else {
        registry_path.join(&manifest_rel_path)
    };

    let component_path = if component_rel_path.is_absolute() {
        component_rel_path
    } else {
        registry_path.join(&component_rel_path)
    };

    // Verify files exist
    if !manifest_path.exists() {
        return Err(
            RegistryError::NotFound(format!("Manifest not found: {:?}", manifest_path)).into(),
        );
    }

    if !component_path.exists() {
        return Err(
            RegistryError::NotFound(format!("Component not found: {:?}", component_path)).into(),
        );
    }

    // Now we need to create a temporary manifest with absolute paths for component_path
    // Read the original manifest
    let manifest_str = fs::read_to_string(&manifest_path).map_err(|_| {
        RegistryError::NotFound(format!("Failed to read manifest file: {:?}", manifest_path)).into()
    })?;

    // Parse the manifest
    let mut manifest: toml::Value = toml::from_str(&manifest_str).map_err(|e| {
        RegistryError::InvalidFormat(format!("Invalid manifest format: {}", e)).into()
    })?;

    // Update component_path to absolute path
    debug!("Original manifest path: {:?}", manifest_path);
    debug!(
        "Original component path from registry: {:?}",
        component_path
    );
    if let Some(component_path_val) = manifest.get_mut("component_path") {
        *component_path_val = toml::Value::String(component_path.to_string_lossy().to_string());
        debug!(
            "Updated component path to: {}",
            component_path.to_string_lossy()
        );
    }

    // Also update init_state to absolute path if it exists
    if let Some(init_state_val) = manifest.get_mut("init_state") {
        if let Some(init_state_str) = init_state_val.as_str() {
            let init_state_path = PathBuf::from(init_state_str);
            if !init_state_path.is_absolute() {
                let abs_init_path = manifest_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(&init_state_path);
                debug!(
                    "Updating init_state from {} to {}",
                    init_state_str,
                    abs_init_path.display()
                );
                *init_state_val = toml::Value::String(abs_init_path.to_string_lossy().to_string());
            }
        }
    }

    // Create a temporary directory for the modified manifest
    let temp_dir = std::env::temp_dir().join("theater_registry");
    fs::create_dir_all(&temp_dir).map_err(|e| {
        RegistryError::RegistryError(format!("Failed to create temp directory: {}", e)).into()
    })?;

    // Write the modified manifest to a temporary file
    let temp_manifest_path = temp_dir.join(format!("{}-{}-actor.toml", name, version_to_use));
    let modified_manifest_str = toml::to_string(&manifest).map_err(|e| {
        RegistryError::RegistryError(format!("Failed to serialize manifest: {}", e)).into()
    })?;

    fs::write(&temp_manifest_path, modified_manifest_str).map_err(|e| {
        RegistryError::RegistryError(format!("Failed to write temporary manifest: {}", e)).into()
    })?;

    debug!(
        "Created temporary manifest with absolute paths at {:?}",
        temp_manifest_path
    );

    Ok((temp_manifest_path, component_path))
}

/// Helper to extract path from config
fn extract_path_from_config(config: &Value, key: &str) -> Result<String> {
    // Get the path from config
    let path_str = config
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            RegistryError::InvalidFormat(format!("{} not found in registry config", key))
        })?;

    Ok(path_str)
}

