# Registry Path and Resource Resolution Proposal

## Description

The Theater system runs and manages actors, each requiring specific resources (components, manifests, and initial states) to function. We've recently completed a change that allows manifests to be passed as either strings or paths, decoupling them from the filesystem. This proposal extends that work by creating a comprehensive registry system with a unified URI scheme for referencing all types of resources.

The goal is to make the Theater system more flexible, portable, and maintainable by abstracting resource references from their physical locations and providing a consistent way to reference and resolve resources across different environments.

## Problem Statement

The current approach has several limitations:

1. **Filesystem Dependency**: Direct filesystem paths make the system less portable and more brittle
2. **Lack of Abstraction**: Resources are tightly coupled to their physical locations
3. **Cross-Platform Issues**: Direct paths cause problems across different operating systems
4. **Integration Challenges**: External systems struggle to interact with the registry without filesystem knowledge
5. **Resource Location Complexity**: As the system grows, maintaining a mental model of resource locations becomes difficult
6. **Component Reusability**: Sharing components across different actors is cumbersome
7. **Versioning Complexity**: Managing multiple versions of components is manual and error-prone
8. **Environment Transitions**: Moving code between development, staging, and production is difficult

## Proposed Solution

We propose creating a comprehensive registry system with a flexible URI scheme for referencing resources. The registry will be an abstraction layer between logical resource references and their physical locations.

### Registry URI Scheme

The primary scheme will be:

```
[registry[@name]][+protocol]::type/category[/version][/resource-path]
```

Where:
- `registry[@name]` optionally specifies a named registry (defaults to primary registry)
- `+protocol` optionally specifies the protocol for remote registries
- `type` is the resource type: `components`, `manifests`, or `states`
- `category` can be an actor name, functional group, or domain
- `version` can be explicit or use semantic tags
- `resource-path` is the specific resource path

Alternative content-based addressing:

```
[registry[@name]][+protocol]::hash:algorithm:digest
```

### Examples

```
# Basic component reference
registry::components/chat/v1.0.0/chat.wasm

# Alternative version syntax
registry::components/chat@v1.0.0/chat.wasm

# Named registry with version tag
registry@staging::components/chat@stable/chat.wasm

# Remote registry
registry+http://example.com::components/auth/v2.1.0/auth.wasm

# Content-addressed component
registry::hash:sha256:a1b2c3d4...

# Manifest in default registry
registry::manifests/chat/production.toml

# Shared state resource
registry::states/shared/empty-chat.json
```

## Implementation Details

### 1. Registry URI Parser

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryUri {
    registry_name: Option<String>,
    protocol: Option<String>,
    addressing_type: AddressingType,
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

impl RegistryUri {
    /// Parse a registry URI string
    pub fn parse(uri: &str) -> Result<Self> {
        // Implementation details...
    }
}
```

### 2. Registry Trait and Implementations

```rust
pub trait Registry {
    /// Resolve a path-based resource reference
    fn resolve_path(
        &self, 
        resource_type: &ResourceType, 
        category: &str, 
        version: Option<&str>, 
        path: &str
    ) -> Result<Resource>;
    
    /// Resolve a hash-based resource reference
    fn resolve_hash(&self, algorithm: &str, digest: &str) -> Result<Resource>;
    
    /// List available resources
    fn list_resources(
        &self, 
        resource_type: Option<&ResourceType>, 
        category: Option<&str>
    ) -> Result<Vec<ResourceInfo>>;
    
    /// Store a resource
    fn store_resource(
        &self,
        resource_type: &ResourceType,
        category: &str,
        version: Option<&str>,
        path: &str,
        content: &[u8]
    ) -> Result<ResourceInfo>;
    
    /// Generate a hash for content
    fn hash_content(&self, content: &[u8]) -> Result<(String, String)>;
}

// Filesystem implementation
pub struct FileSystemRegistry {
    base_path: PathBuf,
    version_resolver: Box<dyn VersionResolver>,
}

// HTTP implementation
pub struct HttpRegistry {
    base_url: String,
    client: reqwest::Client,
    version_resolver: Box<dyn VersionResolver>,
}
```

### 3. Registry Manager

```rust
pub struct RegistryManager {
    registries: HashMap<String, Box<dyn Registry>>,
    default_registry: String,
}

impl RegistryManager {
    /// Create a new registry manager
    pub fn new(config: RegistryConfig) -> Result<Self> {
        // Initialize registries from config
    }
    
    /// Resolve a registry URI to a resource
    pub fn resolve(&self, uri: &str) -> Result<Resource> {
        let parsed_uri = RegistryUri::parse(uri)?;
        // Select registry and delegate resolution
    }
    
    /// List available resources
    pub fn list_resources(
        &self,
        registry_name: Option<&str>,
        resource_type: Option<&ResourceType>,
        category: Option<&str>
    ) -> Result<Vec<ResourceInfo>> {
        // List resources from specified registry
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
    ) -> Result<ResourceInfo> {
        // Store resource in specified registry
    }
}
```

### 4. Version Resolution

```rust
pub trait VersionResolver {
    fn resolve_version(
        &self,
        category: &str,
        version_ref: &str
    ) -> Result<String>;
}

pub struct StandardVersionResolver {
    // Maps category/version_alias to actual version
    version_map: HashMap<String, String>,
    category_latest: HashMap<String, String>,
}
```

### 5. Resource Structure

```rust
pub struct Resource {
    pub content: Vec<u8>,
    pub content_type: String,
    pub metadata: ResourceMetadata,
}

pub struct ResourceMetadata {
    pub resource_type: ResourceType,
    pub category: String,
    pub version: Option<String>,
    pub path: String,
    pub hash: Option<(String, String)>, // (algorithm, digest)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub size: usize,
}

pub struct ResourceInfo {
    pub metadata: ResourceMetadata,
    pub uri: String,
}
```

### 6. Registry Configuration

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub default: String,
    pub locations: HashMap<String, RegistryLocation>,
    pub aliases: HashMap<String, String>, // version aliases
}

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
```

## Integration with Existing Code

### 1. Update ManifestSource and Related Enums

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManifestSource {
    Path(PathBuf),
    Content(String),
    Registry(String), // Registry URI
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentSource {
    Path(PathBuf),
    Registry(String), // Registry URI
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InitialStateSource {
    Path(PathBuf),
    Json(String),
    Registry(String), // Registry URI
    Remote(String),   // URL
}
```

### 2. Modify ManifestConfig

```rust
impl ManifestConfig {
    // Add methods to resolve resources using the registry
    pub fn resolve_resources(
        &mut self, 
        registry_manager: &RegistryManager
    ) -> Result<()> {
        // Resolve component_path if it's a registry URI
        if let ComponentSource::Registry(uri) = &self.component_source {
            let resource = registry_manager.resolve(uri)?;
            // ...
        }
        
        // Resolve init_state if it's a registry URI
        if let Some(InitialStateSource::Registry(uri)) = &self.init_state {
            let resource = registry_manager.resolve(uri)?;
            // ...
        }
        
        Ok(())
    }
}
```

### 3. Update Actor Reference Resolution

```rust
pub fn resolve_actor_reference(
    reference: &str,
    registry_manager: &RegistryManager
) -> Result<(ManifestConfig, ComponentBinary)> {
    // Check if reference is a registry URI
    if reference.contains("registry::") {
        let manifest_resource = registry_manager.resolve(reference)?;
        let manifest_content = std::str::from_utf8(&manifest_resource.content)?;
        let mut manifest_config = ManifestConfig::from_string(manifest_content)?;
        
        // Resolve component and other resources
        manifest_config.resolve_resources(registry_manager)?;
        
        // Load component
        let component_binary = load_component(&manifest_config.component_path)?;
        
        return Ok((manifest_config, component_binary));
    }
    
    // Otherwise, use existing resolution logic
    // ...
}
```

## Registry Operations

### 1. Publishing Resources

```rust
impl RegistryManager {
    /// Publish a component to the registry
    pub fn publish_component(
        &self,
        registry_name: Option<&str>,
        category: &str,
        version: &str,
        name: &str,
        component_binary: &[u8]
    ) -> Result<ResourceInfo> {
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
    ) -> Result<ResourceInfo> {
        self.store_resource(
            registry_name,
            &ResourceType::Manifest,
            category,
            version,
            &format!("{}.toml", name),
            manifest_content.as_bytes()
        )
    }
    
    // Similar methods for states and other resources
}
```

### 2. CLI Commands

```
# Publish a component
theater registry publish component --category chat --version v1.0.0 --file chat.wasm

# Publish a manifest
theater registry publish manifest --category chat --file actor.toml

# List components
theater registry list components --category chat

# Get component info
theater registry info registry::components/chat/v1.0.0/chat.wasm

# Set version alias
theater registry alias chat@stable chat@v1.0.0
```

## Key Questions to Address

Before implementing this proposal, we should consider the following questions:

1. **Resource Immutability**: 
   - Should resources be immutable once published? 
   - How do we handle updates vs. new versions?
   - Should content-addressed resources be treated differently from path-addressed ones?

2. **Caching Strategy**:
   - How do we cache resources from remote registries?
   - What's the invalidation strategy for cached resources?
   - Should we support prefetching resources?

3. **Registry Synchronization**:
   - How do we keep multiple registries in sync?
   - Is there a primary/replica relationship between registries?
   - Should we support pushing resources between registries?

4. **Security Model**:
   - How do we authenticate and authorize access to registries?
   - Should we support signing resources?
   - How do we validate the integrity of resources?

5. **Failure Handling**:
   - What happens when a registry is unavailable?
   - Should we support fallback registries?
   - How do we handle partial failures in multi-registry operations?

6. **Version Management**:
   - How do we determine the "latest" version?
   - Should we support semantic versioning rules?
   - How do we handle version conflicts?

7. **Resource Dependencies**:
   - How do we track and resolve dependencies between resources?
   - Should manifests reference components via registry URIs?
   - How do we handle circular dependencies?

8. **Registry Discovery**:
   - How do users discover available resources?
   - Should we support searching and filtering?
   - How do we handle resource metadata and documentation?

9. **Migration Strategy**:
   - How do we migrate existing resources to the new registry system?
   - What tools do we provide for conversion?
   - How do we handle backward compatibility during the transition?

10. **Performance Considerations**:
    - How do we optimize resource resolution performance?
    - What benchmarks should we establish?
    - How do we monitor and troubleshoot performance issues?

## Implementation Plan

1. **Phase 1: Core Registry Infrastructure**
   - Create the RegistryUri parser
   - Implement the Registry trait
   - Develop the FileSystemRegistry implementation
   - Build the RegistryManager

2. **Phase 2: Resource Resolution**
   - Update ManifestConfig to support registry URIs
   - Modify the actor reference resolver
   - Update the resource loading mechanisms
   - Create the version resolution system

3. **Phase 3: CLI and Management Tools**
   - Develop CLI commands for registry operations
   - Create tools for publishing resources
   - Implement resource listing and discovery
   - Build version management capabilities

4. **Phase 4: Remote Registries and Advanced Features**
   - Implement HttpRegistry and other remote types
   - Develop caching mechanisms
   - Add content-based addressing
   - Build security features

5. **Phase 5: Testing and Documentation**
   - Create comprehensive tests
   - Document the registry system
   - Provide examples and guides
   - Build migration tools

## Benefits

1. **Abstraction**: Resources referenced by logical identifiers rather than physical locations
2. **Portability**: Registry URIs work consistently across environments
3. **Flexibility**: Multiple addressing schemes for different needs
4. **Versioning**: Explicit support for versioning and version aliases
5. **Discoverability**: Resources organized in a structured, discoverable hierarchy
6. **Distribution**: Support for remote registries enables sharing and distribution
7. **Content Integrity**: Content-based addressing ensures resource integrity
8. **Environment Transitions**: Easily move between development, staging, and production

## Risks and Mitigations

1. **Complexity**: The registry system adds complexity
   - Mitigation: Create clear documentation and provide simple interfaces

2. **Performance**: Resolution through registries might impact performance
   - Mitigation: Implement caching and benchmarking

3. **Adoption**: Users must learn and adopt the new URI scheme
   - Mitigation: Provide migration tools and backward compatibility

4. **Security**: Remote registries introduce security concerns
   - Mitigation: Implement content validation and signing

5. **Reliability**: Dependency on external registries increases failure points
   - Mitigation: Support fallbacks and local caching

## Conclusion

The proposed registry system provides a comprehensive solution for resource management in the Theater system. By abstracting resource references from their physical locations and providing a flexible, extensible URI scheme, we can make the system more portable, maintainable, and scalable.

Implementation should proceed in phases, starting with the core registry infrastructure and resource resolution, then adding management tools and advanced features. Throughout the process, we should gather feedback and adapt the design to meet practical needs.

The registry system represents a significant architectural improvement that will enable new use cases and workflows while making existing ones more robust and flexible.
