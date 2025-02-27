use crate::Result;
use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UriError {
    #[error("Invalid registry URI format: {0}")]
    InvalidFormat(String),
    #[error("Unsupported addressing type: {0}")]
    UnsupportedAddressing(String),
    #[error("Invalid resource type: {0}")]
    InvalidResourceType(String),
    #[error("Missing required component: {0}")]
    MissingComponent(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryUri {
    pub registry_name: Option<String>,
    pub protocol: Option<String>,
    pub addressing_type: AddressingType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressingType {
    Path {
        resource_type: ResourceType,
        category: String,
        version: Option<String>,
        path: String,
    },
    Hash {
        algorithm: String,
        digest: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    Component,
    Manifest,
    State,
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceType::Component => write!(f, "components"),
            ResourceType::Manifest => write!(f, "manifests"),
            ResourceType::State => write!(f, "states"),
        }
    }
}

impl ResourceType {
    pub fn from_str(s: &str) -> Result<Self, UriError> {
        match s {
            "components" => Ok(ResourceType::Component),
            "manifests" => Ok(ResourceType::Manifest),
            "states" => Ok(ResourceType::State),
            _ => Err(UriError::InvalidResourceType(s.to_string())),
        }
    }
}

impl RegistryUri {
    /// Parse a registry URI string
    pub fn parse(uri: &str) -> Result<Self, UriError> {
        // Check if this is a registry URI
        if !uri.contains("registry") {
            return Err(UriError::InvalidFormat(format!(
                "URI must start with 'registry': {}",
                uri
            )));
        }

        // Split into registry part and resource part
        let parts: Vec<&str> = uri.split("::").collect();
        if parts.len() != 2 {
            return Err(UriError::InvalidFormat(format!(
                "URI must have '::' separator: {}",
                uri
            )));
        }

        let registry_part = parts[0];
        let resource_part = parts[1];

        // Parse registry part
        let (registry_name, protocol) = Self::parse_registry_part(registry_part)?;

        // Parse resource part
        let addressing_type = Self::parse_resource_part(resource_part)?;

        Ok(RegistryUri {
            registry_name,
            protocol,
            addressing_type,
        })
    }

    /// Parse the registry part of the URI (e.g., "registry@name+protocol")
    fn parse_registry_part(registry_part: &str) -> Result<(Option<String>, Option<String>), UriError> {
        // Check if there's a registry name
        let registry_name = if registry_part.contains('@') {
            let parts: Vec<&str> = registry_part.split('@').collect();
            if parts.len() != 2 {
                return Err(UriError::InvalidFormat(format!(
                    "Invalid registry name format: {}",
                    registry_part
                )));
            }
            Some(parts[1].split('+').next().unwrap_or("").to_string())
        } else {
            None
        };

        // Check if there's a protocol
        let protocol = if registry_part.contains('+') {
            let parts: Vec<&str> = registry_part.split('+').collect();
            if parts.len() != 2 {
                return Err(UriError::InvalidFormat(format!(
                    "Invalid protocol format: {}",
                    registry_part
                )));
            }
            Some(parts[1].to_string())
        } else {
            None
        };

        Ok((registry_name, protocol))
    }

    /// Parse the resource part of the URI (path or hash based)
    fn parse_resource_part(resource_part: &str) -> Result<AddressingType, UriError> {
        // Check if this is a hash-based reference
        if resource_part.starts_with("hash:") {
            return Self::parse_hash_resource(resource_part);
        }

        // Otherwise it's a path-based reference
        Self::parse_path_resource(resource_part)
    }

    /// Parse a hash-based resource reference (e.g., "hash:sha256:abc123")
    fn parse_hash_resource(resource_part: &str) -> Result<AddressingType, UriError> {
        let parts: Vec<&str> = resource_part.split(':').collect();
        if parts.len() != 3 || parts[0] != "hash" {
            return Err(UriError::InvalidFormat(format!(
                "Invalid hash format, expected 'hash:algorithm:digest': {}",
                resource_part
            )));
        }

        Ok(AddressingType::Hash {
            algorithm: parts[1].to_string(),
            digest: parts[2].to_string(),
        })
    }

    /// Parse a path-based resource reference (e.g., "components/chat/v1.0.0/chat.wasm")
    fn parse_path_resource(resource_part: &str) -> Result<AddressingType, UriError> {
        let parts: Vec<&str> = resource_part.split('/').collect();
        if parts.len() < 2 {
            return Err(UriError::InvalidFormat(format!(
                "Invalid path format, expected at least 'type/category': {}",
                resource_part
            )));
        }

        // First part is the resource type
        let resource_type = ResourceType::from_str(parts[0])?;

        // Second part is the category
        let category = parts[1].to_string();

        // Check if the third part is a version or part of the path
        let (version, path) = if parts.len() >= 3 {
            // Handle version formats with @ symbol as well
            if parts[1].contains('@') {
                let category_parts: Vec<&str> = parts[1].split('@').collect();
                let cat = category_parts[0].to_string();
                let ver = Some(category_parts[1].to_string());
                let path_parts = &parts[2..];
                (ver, cat.to_string() + "/" + &path_parts.join("/"))
            } else if parts[2].starts_with('v') || parts[2].contains('.') {
                // This is likely a version
                let path_parts = if parts.len() > 3 { &parts[3..] } else { &[] };
                (Some(parts[2].to_string()), path_parts.join("/"))
            } else {
                // This is part of the path
                let path_parts = &parts[2..];
                (None, path_parts.join("/"))
            }
        } else {
            (None, String::new())
        };

        Ok(AddressingType::Path {
            resource_type,
            category,
            version,
            path,
        })
    }

    /// Convert the URI to a string
    pub fn to_string(&self) -> String {
        let registry_part = match (&self.registry_name, &self.protocol) {
            (Some(name), Some(protocol)) => format!("registry@{}+{}", name, protocol),
            (Some(name), None) => format!("registry@{}", name),
            (None, Some(protocol)) => format!("registry+{}", protocol),
            (None, None) => "registry".to_string(),
        };

        let resource_part = match &self.addressing_type {
            AddressingType::Path {
                resource_type,
                category,
                version,
                path,
            } => {
                let mut result = format!("{}/{}", resource_type, category);
                if let Some(ver) = version {
                    result = format!("{}/{}", result, ver);
                }
                if !path.is_empty() {
                    result = format!("{}/{}", result, path);
                }
                result
            }
            AddressingType::Hash {
                algorithm,
                digest,
            } => {
                format!("hash:{}:{}", algorithm, digest)
            }
        };

        format!("{}::{}", registry_part, resource_part)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_uri_parsing() {
        let uri = "registry::components/chat/v1.0.0/chat.wasm";
        let parsed = RegistryUri::parse(uri).unwrap();
        
        assert_eq!(parsed.registry_name, None);
        assert_eq!(parsed.protocol, None);
        
        if let AddressingType::Path {
            resource_type,
            category,
            version,
            path,
        } = parsed.addressing_type {
            assert_eq!(resource_type, ResourceType::Component);
            assert_eq!(category, "chat");
            assert_eq!(version, Some("v1.0.0".to_string()));
            assert_eq!(path, "chat.wasm");
        } else {
            panic!("Expected PathAddressing");
        }
    }

    #[test]
    fn test_named_registry_uri_parsing() {
        let uri = "registry@staging::components/chat/v1.0.0/chat.wasm";
        let parsed = RegistryUri::parse(uri).unwrap();
        
        assert_eq!(parsed.registry_name, Some("staging".to_string()));
        assert_eq!(parsed.protocol, None);
        
        if let AddressingType::Path {
            resource_type,
            category,
            version,
            path,
        } = parsed.addressing_type {
            assert_eq!(resource_type, ResourceType::Component);
            assert_eq!(category, "chat");
            assert_eq!(version, Some("v1.0.0".to_string()));
            assert_eq!(path, "chat.wasm");
        } else {
            panic!("Expected PathAddressing");
        }
    }

    #[test]
    fn test_protocol_uri_parsing() {
        let uri = "registry+http://example.com::components/auth/v2.1.0/auth.wasm";
        let parsed = RegistryUri::parse(uri).unwrap();
        
        assert_eq!(parsed.registry_name, None);
        assert_eq!(parsed.protocol, Some("http://example.com".to_string()));
        
        if let AddressingType::Path {
            resource_type,
            category,
            version,
            path,
        } = parsed.addressing_type {
            assert_eq!(resource_type, ResourceType::Component);
            assert_eq!(category, "auth");
            assert_eq!(version, Some("v2.1.0".to_string()));
            assert_eq!(path, "auth.wasm");
        } else {
            panic!("Expected PathAddressing");
        }
    }

    #[test]
    fn test_hash_uri_parsing() {
        let uri = "registry::hash:sha256:a1b2c3d4e5f6g7h8i9j0";
        let parsed = RegistryUri::parse(uri).unwrap();
        
        assert_eq!(parsed.registry_name, None);
        assert_eq!(parsed.protocol, None);
        
        if let AddressingType::Hash {
            algorithm,
            digest,
        } = parsed.addressing_type {
            assert_eq!(algorithm, "sha256");
            assert_eq!(digest, "a1b2c3d4e5f6g7h8i9j0");
        } else {
            panic!("Expected HashAddressing");
        }
    }

    #[test]
    fn test_uri_to_string() {
        let uri = "registry@staging::components/chat/v1.0.0/chat.wasm";
        let parsed = RegistryUri::parse(uri).unwrap();
        assert_eq!(parsed.to_string(), uri);
        
        let uri = "registry::hash:sha256:a1b2c3d4e5f6g7h8i9j0";
        let parsed = RegistryUri::parse(uri).unwrap();
        assert_eq!(parsed.to_string(), uri);
    }
}
