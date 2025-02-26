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
fn resolve_direct_actor_path(path: &Path) -> Result<(PathBuf, PathBuf)> {
    if !path.exists() {
        return Err(Error::NotFound(format!(
            "Actor manifest not found: {:?}",
            path
        )));
    }

    // Read manifest to extract component path
    let manifest_str = fs::read_to_string(path)?;
    let manifest: Value = toml::from_str(&manifest_str)?;

    let component_path = if let Some(component) = manifest.get("component_path") {
        if let Some(component_str) = component.as_str() {
            let component_path = PathBuf::from(component_str);

            // If component path is relative, resolve it relative to manifest directory
            if !component_path.is_absolute() {
                if let Some(parent) = path.parent() {
                    parent.join(component_path)
                } else {
                    component_path
                }
            } else {
                component_path
            }
        } else {
            return Err(Error::InvalidFormat(
                "component_path is not a string".to_string(),
            ));
        }
    } else {
        return Err(Error::InvalidFormat(
            "component_path not found in manifest".to_string(),
        ));
    };

    Ok((path.to_path_buf(), component_path))
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
fn resolve_registry_actor(reference: &str, registry_path: &Path) -> Result<(PathBuf, PathBuf)> {
    // Parse the reference
    let (name, version) = parse_actor_reference(reference);

    // Get paths to components and manifests
    let config_path = registry_path.join("config.toml");
    let config_str = fs::read_to_string(&config_path)
        .map_err(|_| Error::NotFound(format!("Registry config not found: {:?}", config_path)))?;

    let config: Value = toml::from_str(&config_str)?;

    // Extract directories from config
    let manifest_dir = extract_path_from_config(&config, "manifest_dir")?;
    let component_dir = extract_path_from_config(&config, "component_dir")?;

    // Load the index to get version info
    let index_path = registry_path.join("index.toml");
    let index_str = fs::read_to_string(&index_path)
        .map_err(|_| Error::NotFound(format!("Registry index not found: {:?}", index_path)))?;

    let index: Value = toml::from_str(&index_str)?;

    // Find the actor in the index
    let actors = index
        .get("actors")
        .and_then(|a| a.as_array())
        .ok_or_else(|| {
            Error::InvalidFormat("actors list not found in registry index".to_string())
        })?;

    let actor = actors
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(&name))
        .ok_or_else(|| Error::NotFound(format!("Actor '{}' not found in registry", name)))?;

    // Determine which version to use
    let version_to_use = match version {
        Some(v) => {
            // Verify this version exists
            let versions = actor
                .get("versions")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    Error::InvalidFormat("versions list not found for actor".to_string())
                })?;

            let version_exists = versions.iter().any(|ver| ver.as_str() == Some(&v));

            if !version_exists {
                return Err(Error::NotFound(format!(
                    "Version '{}' not found for actor '{}'",
                    v, name
                )));
            }

            v
        }
        None => {
            // Use latest version
            actor
                .get("latest")
                .and_then(|l| l.as_str())
                .ok_or_else(|| {
                    Error::InvalidFormat("latest version not found for actor".to_string())
                })?
                .to_string()
        }
    };

    // Construct paths to manifest and component
    let manifest_path = PathBuf::from(manifest_dir)
        .join(&name)
        .join(&version_to_use)
        .join("actor.toml");

    let component_path = PathBuf::from(component_dir)
        .join(&name)
        .join(&version_to_use)
        .join(format!("{}.wasm", name));

    // Verify files exist
    if !manifest_path.exists() {
        return Err(Error::NotFound(format!(
            "Manifest not found: {:?}",
            manifest_path
        )));
    }

    if !component_path.exists() {
        return Err(Error::NotFound(format!(
            "Component not found: {:?}",
            component_path
        )));
    }

    debug!(
        "Resolved actor '{}:{}' to manifest {:?} and component {:?}",
        name, version_to_use, manifest_path, component_path
    );

    Ok((manifest_path, component_path))
}

/// Helper to extract path from config
fn extract_path_from_config(config: &Value, key: &str) -> Result<String> {
    config
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::InvalidFormat(format!("{} not found in registry config", key)))
}
