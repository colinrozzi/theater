use crate::registry::uri::{AddressingType, RegistryUri, ResourceType, UriError};
use crate::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use sha2::{Sha256, Digest};
use std::io::Read;

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Registry not found: {0}")]
    NotFound(String),
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    #[error("Registry error: {0}")]
    RegistryError(String),
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    #[error("Version error: {0}")]
    VersionError(String),
    #[error("URI error: {0}")]
    UriError(#[from] UriError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Resource exists: {0}")]
    ResourceExists(String),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("WalkDir error: {0}")]
    WalkDirError(#[from] walkdir::Error),
}

/// Resource structure representing a resolved resource from a registry
#[derive(Debug, Clone)]
pub struct Resource {
    pub content: Vec<u8>,
    pub content_type: String,
    pub metadata: ResourceMetadata,
}

/// Metadata for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetadata {
    pub resource_type: String, // "component", "manifest", "state"
    pub category: String,
    pub version: Option<String>,
    pub path: String,
    pub hash: Option<(String, String)>, // (algorithm, digest)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub size: usize,
}

/// Information about a resource (without content)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub metadata: ResourceMetadata,
    pub uri: String,
}

/// Registry configuration for a location
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RegistryLocation {
    #[serde(rename = "filesystem")]
    FileSystem { path: PathBuf },
    
    #[serde(rename = "http")]
    Http { url: String },
    
    #[serde(rename = "s3")]
    S3 { bucket: String, prefix: Option<String> },
}

/// Full registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub default: String,
    pub locations: HashMap<String, RegistryLocation>,
    pub aliases: HashMap<String, String>, // version aliases
}

/// Interface for registries
pub trait Registry {
    /// Resolve a path-based resource reference
    fn resolve_path(
        &self, 
        resource_type: &ResourceType, 
        category: &str, 
        version: Option<&str>, 
        path: &str
    ) -> Result<Resource, RegistryError>;
    
    /// Resolve a hash-based resource reference
    fn resolve_hash(&self, algorithm: &str, digest: &str) -> Result<Resource, RegistryError>;
    
    /// List available resources
    fn list_resources(
        &self, 
        resource_type: Option<&ResourceType>, 
        category: Option<&str>
    ) -> Result<Vec<ResourceInfo>, RegistryError>;
    
    /// Store a resource
    fn store_resource(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version: Option<&str>,
        path: &str,
        content: &[u8]
    ) -> Result<ResourceInfo, RegistryError>;
    
    /// Generate a hash for content
    fn hash_content(&self, content: &[u8]) -> Result<(String, String), RegistryError>;

    /// Resolve a registry URI
    fn resolve_uri(&self, uri: &RegistryUri) -> Result<Resource, RegistryError> {
        match &uri.addressing_type {
            AddressingType::Path {
                resource_type,
                category,
                version,
                path,
            } => {
                self.resolve_path(
                    &resource_type,
                    &category,
                    version.as_deref(),
                    &path,
                )
            }
            AddressingType::Hash {
                algorithm,
                digest,
            } => {
                self.resolve_hash(&algorithm, &digest)
            }
        }
    }
}

/// Registry Manager for handling multiple registries
pub struct RegistryManager {
    registries: HashMap<String, Box<dyn Registry>>,
    default_registry: String,
}

impl RegistryManager {
    /// Create a new registry manager
    pub fn new(config: RegistryConfig) -> Result<Self, RegistryError> {
        let mut registries = HashMap::new();
        
        // Create registry instances for each location
        for (name, location) in &config.locations {
            let registry: Box<dyn Registry> = match location {
                RegistryLocation::FileSystem { path } => {
                    Box::new(FileSystemRegistry::new(path.clone())?)
                }
                RegistryLocation::Http { url: _ } => {
                    // We'll implement this later
                    return Err(RegistryError::RegistryError(
                        "HTTP registry not implemented yet".to_string()
                    ));
                }
                RegistryLocation::S3 { bucket: _, prefix: _ } => {
                    // We'll implement this later
                    return Err(RegistryError::RegistryError(
                        "S3 registry not implemented yet".to_string()
                    ));
                }
            };
            
            registries.insert(name.clone(), registry);
        }
        
        // Check that default registry exists
        if !registries.contains_key(&config.default) {
            return Err(RegistryError::NotFound(format!(
                "Default registry '{}' not found in configuration",
                config.default
            )));
        }
        
        Ok(Self {
            registries,
            default_registry: config.default,
        })
    }
    
    /// Resolve a registry URI to a resource
    pub fn resolve(&self, uri_str: &str) -> Result<Resource, RegistryError> {
        let uri = RegistryUri::parse(uri_str)?;
        
        // Determine which registry to use
        let registry_name = uri.registry_name.as_deref().unwrap_or(&self.default_registry);
        
        // Get the registry
        let registry = self.registries.get(registry_name).ok_or_else(|| {
            RegistryError::NotFound(format!("Registry '{}' not found", registry_name))
        })?;
        
        // Delegate to the registry
        registry.resolve_uri(&uri)
    }
    
    /// List available resources
    pub fn list_resources(
        &self,
        registry_name: Option<&str>,
        resource_type: Option<&ResourceType>,
        category: Option<&str>
    ) -> Result<Vec<ResourceInfo>, RegistryError> {
        // Determine which registry to use
        let registry_name = registry_name.unwrap_or(&self.default_registry);
        
        // Get the registry
        let registry = self.registries.get(registry_name).ok_or_else(|| {
            RegistryError::NotFound(format!("Registry '{}' not found", registry_name))
        })?;
        
        // Delegate to the registry
        registry.list_resources(resource_type, category)
    }
    
    /// Store a resource
    pub fn store_resource(
        &self,
        registry_name: Option<&str>,
        resource_type: &ResourceType,
        category: &str,
        version: Option<&str>,
        path: &str,
        content: &[u8]
    ) -> Result<ResourceInfo, RegistryError> {
        // Determine which registry to use
        let registry_name = registry_name.unwrap_or(&self.default_registry);
        
        // Get the registry
        let registry = self.registries.get(registry_name).ok_or_else(|| {
            RegistryError::NotFound(format!("Registry '{}' not found", registry_name))
        })?;
        
        // Delegate to the registry
        registry.store_resource(resource_type, category, version, path, content)
    }
    
    /// Get a list of registry names
    pub fn registry_names(&self) -> Vec<String> {
        self.registries.keys().cloned().collect()
    }
    
    /// Get the default registry name
    pub fn default_registry(&self) -> &str {
        &self.default_registry
    }
    
    /// Publish a component to the registry
    pub fn publish_component(
        &self,
        registry_name: Option<&str>,
        category: &str,
        version: &str,
        name: &str,
        component_binary: &[u8]
    ) -> Result<ResourceInfo, RegistryError> {
        self.store_resource(
            registry_name,
            &ResourceType::Component,
            category,
            Some(version),
            &format!("{}.wasm", name),
            component_binary
        )
    }
    
    /// Publish a manifest to the registry
    pub fn publish_manifest(
        &self,
        registry_name: Option<&str>,
        category: &str,
        version: Option<&str>,
        name: &str,
        manifest_content: &str
    ) -> Result<ResourceInfo, RegistryError> {
        self.store_resource(
            registry_name,
            &ResourceType::Manifest,
            category,
            version,
            &format!("{}.toml", name),
            manifest_content.as_bytes()
        )
    }
}

/// FileSystem Registry implementation
pub struct FileSystemRegistry {
    base_path: PathBuf,
}

impl FileSystemRegistry {
    /// Create a new file system registry
    pub fn new(base_path: PathBuf) -> Result<Self, RegistryError> {
        // Create directories if they don't exist
        let registry = Self { base_path };

        // Make sure the base directory exists
        std::fs::create_dir_all(&registry.base_path).map_err(|e| {
            RegistryError::RegistryError(format!("Failed to create registry directory: {}", e))
        })?;

        // Create subdirectories for each resource type
        std::fs::create_dir_all(registry.resource_type_path(&ResourceType::Component)).map_err(|e| {
            RegistryError::RegistryError(format!("Failed to create components directory: {}", e))
        })?;

        std::fs::create_dir_all(registry.resource_type_path(&ResourceType::Manifest)).map_err(|e| {
            RegistryError::RegistryError(format!("Failed to create manifests directory: {}", e))
        })?;

        std::fs::create_dir_all(registry.resource_type_path(&ResourceType::State)).map_err(|e| {
            RegistryError::RegistryError(format!("Failed to create states directory: {}", e))
        })?;

        Ok(registry)
    }

    /// Get the path for a resource type
    fn resource_type_path(&self, resource_type: &ResourceType) -> PathBuf {
        self.base_path.join(resource_type.to_string())
    }

    /// Get the path for a category within a resource type
    fn category_path(&self, resource_type: &ResourceType, category: &str) -> PathBuf {
        self.resource_type_path(resource_type).join(category)
    }

    /// Get the path for a versioned category
    fn versioned_category_path(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version: &str,
    ) -> PathBuf {
        self.category_path(resource_type, category).join(version)
    }

    /// Resolve a version reference, handling "latest"
    fn resolve_version(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version_ref: Option<&str>,
    ) -> Result<String, RegistryError> {
        match version_ref {
            Some("latest") | None => {
                // Find the latest version by listing the category directory
                let category_path = self.category_path(resource_type, category);
                if !category_path.exists() {
                    return Err(RegistryError::ResourceNotFound(format!(
                        "Category '{}' not found",
                        category
                    )));
                }

                let mut versions = Vec::new();
                for entry in std::fs::read_dir(category_path)? {
                    let entry = entry?;
                    if entry.file_type()?.is_dir() {
                        let version = entry.file_name().to_string_lossy().to_string();
                        versions.push(version);
                    }
                }

                if versions.is_empty() {
                    return Err(RegistryError::ResourceNotFound(format!(
                        "No versions found for category '{}'",
                        category
                    )));
                }

                // Sort versions and take the last one
                versions.sort_by(|a, b| {
                    // Try to parse as semver
                    let a_semver = semver::Version::parse(a);
                    let b_semver = semver::Version::parse(b);

                    match (a_semver, b_semver) {
                        (Ok(a_ver), Ok(b_ver)) => a_ver.cmp(&b_ver),
                        _ => a.cmp(b), // Fallback to lexical comparison
                    }
                });

                Ok(versions.last().unwrap().clone())
            }
            Some(ver) => Ok(ver.to_string()),
        }
    }

    /// Create a resource metadata
    fn create_metadata(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version: Option<&str>,
        path: &str,
        content: &[u8],
    ) -> Result<ResourceMetadata, RegistryError> {
        // Generate a hash for the content
        let (algorithm, digest) = self.hash_content(content)?;

        Ok(ResourceMetadata {
            resource_type: resource_type.to_string(),
            category: category.to_string(),
            version: version.map(|v| v.to_string()),
            path: path.to_string(),
            hash: Some((algorithm, digest)),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            size: content.len(),
        })
    }
}

impl Registry for FileSystemRegistry {
    fn resolve_path(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version: Option<&str>,
        path: &str,
    ) -> Result<Resource, RegistryError> {
        // Resolve the version
        let resolved_version = self.resolve_version(resource_type, category, version)?;

        // Construct the path to the resource
        let resource_path = self
            .versioned_category_path(resource_type, category, &resolved_version)
            .join(path);

        if !resource_path.exists() {
            return Err(RegistryError::ResourceNotFound(format!(
                "Resource not found: {:?}",
                resource_path
            )));
        }

        // Read the resource content
        let mut file = std::fs::File::open(&resource_path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        // Determine content type
        let content_type = match resource_path.extension() {
            Some(ext) if ext == "wasm" => "application/wasm".to_string(),
            Some(ext) if ext == "toml" => "application/toml".to_string(),
            Some(ext) if ext == "json" => "application/json".to_string(),
            _ => "application/octet-stream".to_string(),
        };

        // Read metadata if it exists
        let metadata_path = resource_path.with_extension("meta.json");
        let metadata = if metadata_path.exists() {
            let metadata_content = std::fs::read_to_string(&metadata_path)?;
            serde_json::from_str(&metadata_content)?
        } else {
            // Create metadata on the fly
            self.create_metadata(resource_type, category, Some(&resolved_version), path, &content)?
        };

        Ok(Resource {
            content,
            content_type,
            metadata,
        })
    }

    fn resolve_hash(&self, algorithm: &str, digest: &str) -> Result<Resource, RegistryError> {
        // For now, we'll implement a basic version that scans all resources
        // A more optimized version would use a hash index

        // Function to check if a resource matches the hash
        let matches_hash = |content: &[u8]| -> bool {
            // Calculate hash using the specified algorithm
            if algorithm == "sha256" {
                let mut hasher = Sha256::new();
                hasher.update(content);
                let result = hasher.finalize();
                let calculated_hash = hex::encode(result);
                calculated_hash == digest
            } else {
                false // Only support sha256 for now
            }
        };

        // Scan all resource types
        for resource_type in &[
            ResourceType::Component,
            ResourceType::Manifest,
            ResourceType::State,
        ] {
            let type_path = self.resource_type_path(resource_type);
            if !type_path.exists() {
                continue;
            }

            // Scan categories
            for category_entry in std::fs::read_dir(type_path)? {
                let category_entry = category_entry?;
                if !category_entry.file_type()?.is_dir() {
                    continue;
                }

                let category = category_entry.file_name().to_string_lossy().to_string();
                let category_path = self.category_path(resource_type, &category);

                // Scan versions
                for version_entry in std::fs::read_dir(category_path)? {
                    let version_entry = version_entry?;
                    if !version_entry.file_type()?.is_dir() {
                        continue;
                    }

                    let version = version_entry.file_name().to_string_lossy().to_string();
                    let version_path = self.versioned_category_path(
                        resource_type,
                        &category,
                        &version,
                    );

                    // Scan files
                    for file_entry in walkdir::WalkDir::new(&version_path) {
                        let file_entry = file_entry?;
                        if !file_entry.file_type().is_file() {
                            continue;
                        }

                        // Skip metadata files
                        if file_entry
                            .path()
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            == "meta.json"
                        {
                            continue;
                        }

                        // Read the file and check if it matches the hash
                        let mut file = std::fs::File::open(file_entry.path())?;
                        let mut content = Vec::new();
                        file.read_to_end(&mut content)?;

                        if matches_hash(&content) {
                            // Found a match
                            let relative_path = file_entry
                                .path()
                                .strip_prefix(&version_path)
                                .unwrap_or(file_entry.path())
                                .to_string_lossy()
                                .to_string();

                            // Determine content type
                            let content_type = match file_entry.path().extension() {
                                Some(ext) if ext == "wasm" => "application/wasm".to_string(),
                                Some(ext) if ext == "toml" => "application/toml".to_string(),
                                Some(ext) if ext == "json" => "application/json".to_string(),
                                _ => "application/octet-stream".to_string(),
                            };

                            // Create metadata
                            let metadata = self.create_metadata(
                                resource_type,
                                &category,
                                Some(&version),
                                &relative_path,
                                &content,
                            )?;

                            return Ok(Resource {
                                content,
                                content_type,
                                metadata,
                            });
                        }
                    }
                }
            }
        }

        Err(RegistryError::ResourceNotFound(format!(
            "No resource found with hash {}:{}",
            algorithm, digest
        )))
    }

    fn list_resources(
        &self,
        resource_type: Option<&ResourceType>,
        category: Option<&str>,
    ) -> Result<Vec<ResourceInfo>, RegistryError> {
        let mut results = Vec::new();

        // Determine which resource types to list
        let resource_types = match resource_type {
            Some(rt) => vec![rt.clone()],
            None => vec![
                ResourceType::Component,
                ResourceType::Manifest,
                ResourceType::State,
            ],
        };

        for rt in &resource_types {
            let type_path = self.resource_type_path(rt);
            if !type_path.exists() {
                continue;
            }

            // List categories or filter by the specified category
            let categories = match category {
                Some(cat) => {
                    let cat_path = self.category_path(rt, cat);
                    if !cat_path.exists() {
                        continue;
                    }
                    vec![cat.to_string()]
                }
                None => {
                    let mut cats = Vec::new();
                    for entry in std::fs::read_dir(type_path)? {
                        let entry = entry?;
                        if entry.file_type()?.is_dir() {
                            cats.push(entry.file_name().to_string_lossy().to_string());
                        }
                    }
                    cats
                }
            };

            for cat in &categories {
                let category_path = self.category_path(rt, cat);

                // List versions
                for version_entry in std::fs::read_dir(category_path)? {
                    let version_entry = version_entry?;
                    if version_entry.file_type()?.is_dir() {
                        let version = version_entry.file_name().to_string_lossy().to_string();
                        let version_path = self.versioned_category_path(rt, cat, &version);

                        // List files
                        for file_entry in walkdir::WalkDir::new(&version_path) {
                            let file_entry = file_entry?;
                            if !file_entry.file_type().is_file() {
                                continue;
                            }

                            // Skip metadata files
                            if file_entry
                                .path()
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                == "meta.json"
                            {
                                continue;
                            }

                            let relative_path = file_entry
                                .path()
                                .strip_prefix(&version_path)
                                .unwrap_or(file_entry.path())
                                .to_string_lossy()
                                .to_string();

                            // Read or create metadata
                            let metadata_path = file_entry.path().with_extension("meta.json");
                            let metadata = if metadata_path.exists() {
                                let metadata_content = std::fs::read_to_string(&metadata_path)?;
                                serde_json::from_str(&metadata_content)?
                            } else {
                                // Read the file and create metadata
                                let mut file = std::fs::File::open(file_entry.path())?;
                                let mut content = Vec::new();
                                file.read_to_end(&mut content)?;

                                self.create_metadata(rt, cat, Some(&version), &relative_path, &content)?
                            };

                            // Create URI for the resource
                            let uri = RegistryUri {
                                registry_name: None,
                                protocol: None,
                                addressing_type: AddressingType::Path {
                                    resource_type: rt.clone(),
                                    category: cat.clone(),
                                    version: Some(version.clone()),
                                    path: relative_path,
                                },
                            };

                            results.push(ResourceInfo {
                                metadata,
                                uri: uri.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn store_resource(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version: Option<&str>,
        path: &str,
        content: &[u8],
    ) -> Result<ResourceInfo, RegistryError> {
        // Make sure we have a version
        let version = version.unwrap_or("latest");

        // Create directory structure
        let target_dir = self.versioned_category_path(resource_type, category, version);
        std::fs::create_dir_all(&target_dir)?;

        // Create the full path
        let resource_path = target_dir.join(path);

        // Check if the resource already exists
        if resource_path.exists() {
            return Err(RegistryError::ResourceExists(format!(
                "Resource already exists: {:?}",
                resource_path
            )));
        }

        // Create parent directories if needed
        if let Some(parent) = resource_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write the content
        std::fs::write(&resource_path, content)?;

        // Create metadata
        let metadata = self.create_metadata(resource_type, category, Some(version), path, content)?;
        let metadata_path = resource_path.with_extension("meta.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        std::fs::write(&metadata_path, metadata_json)?;

        // Create URI for the resource
        let uri = RegistryUri {
            registry_name: None,
            protocol: None,
            addressing_type: AddressingType::Path {
                resource_type: resource_type.clone(),
                category: category.to_string(),
                version: Some(version.to_string()),
                path: path.to_string(),
            },
        };

        Ok(ResourceInfo {
            metadata,
            uri: uri.to_string(),
        })
    }

    fn hash_content(&self, content: &[u8]) -> Result<(String, String), RegistryError> {
        // Use SHA-256 for hashing
        let mut hasher = Sha256::new();
        hasher.update(content);
        let result = hasher.finalize();
        let hash = hex::encode(result);

        Ok(("sha256".to_string(), hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_filesystem_registry_basics() {
        let temp_dir = tempdir().unwrap();
        let registry = FileSystemRegistry::new(temp_dir.path().to_path_buf()).unwrap();

        // Store a component
        let content = b"mock wasm binary";
        let info = registry
            .store_resource(
                &ResourceType::Component,
                "test",
                Some("v1.0.0"),
                "test.wasm",
                content,
            )
            .unwrap();

        assert_eq!(info.metadata.resource_type, "components");
        assert_eq!(info.metadata.category, "test");
        assert_eq!(info.metadata.version, Some("v1.0.0".to_string()));
        assert_eq!(info.metadata.path, "test.wasm");

        // Resolve by path
        let resource = registry
            .resolve_path(
                &ResourceType::Component,
                "test",
                Some("v1.0.0"),
                "test.wasm",
            )
            .unwrap();

        assert_eq!(resource.content, content);
        assert_eq!(resource.content_type, "application/wasm");

        // Resolve by hash
        let (algorithm, digest) = registry.hash_content(content).unwrap();
        let resource = registry.resolve_hash(&algorithm, &digest).unwrap();

        assert_eq!(resource.content, content);
    }
}
