# Registry URI Implementation

This document describes the implementation of the registry URI system in Theater, which provides a unified way to reference and resolve resources (components, manifests, and states) across different environments.

## Overview

The registry URI system provides:

1. A consistent URI scheme for referencing resources
2. Abstraction from physical file locations
3. Support for multiple registries
4. Version management
5. Content-based addressing

## URI Scheme

The primary scheme is:

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

## Examples

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

## Implementation

The registry URI implementation consists of the following components:

1. `RegistryUri` - Parser and formatter for registry URIs
2. `Registry` trait - Interface for registry implementations
3. `FileSystemRegistry` - Implementation for local file-based registries
4. `RegistryManager` - Coordinates multiple registries
5. Integration with existing Theater systems

### Registry Configuration

Registry configuration is stored in a TOML file:

```toml
# Example registry configuration
default = "local"

[locations.local]
type = "filesystem"
path = "./registry"

[locations.staging]
type = "filesystem"
path = "/var/theater/staging"

[locations.production]
type = "http"
url = "https://registry.example.com"

[aliases]
"latest" = "v1.2.3"
"stable" = "v1.2.0"
```

### CLI Commands

The registry URI system provides the following CLI commands:

```
# Create a registry configuration
theater registry-uri create-config --path .

# List resources in a registry
theater registry-uri list --config registry-config.toml --resource-type components

# Publish a component
theater registry-uri publish-component --config registry-config.toml --component path/to/component.wasm --category chat --version v1.0.0

# Publish a manifest
theater registry-uri publish-manifest --config registry-config.toml --manifest path/to/manifest.toml --category chat --version v1.0.0
```

## Integration with Actor Configuration

The registry URI system integrates with the existing actor configuration system:

### Component Source

The `ManifestConfig` now supports registry URIs through the `ComponentSource` enum:

```rust
pub enum ComponentSource {
    Path(PathBuf),
    Registry(String), // Registry URI
}
```

Example manifest:

```toml
name = "chat-actor"
component_source = "registry::components/chat/v1.0.0/chat.wasm"
```

### Initial State

The `InitialStateSource` now supports registry URIs:

```rust
pub enum InitialStateSource {
    Path(PathBuf),
    Json(String),
    Remote(String),
    Registry(String), // Registry URI
}
```

Example manifest:

```toml
name = "chat-actor"
component_source = "registry::components/chat/v1.0.0/chat.wasm"
init_state = "registry::states/chat/empty.json"
```

## Resource Resolution

When an actor is started with a registry URI reference, the following happens:

1. The URI is parsed and validated
2. The appropriate registry is selected
3. The resource is resolved to a temporary file
4. The temporary file is used for component instantiation

This process is transparent to the rest of the system, which continues to work with file paths as before.

## Benefits

1. **Abstraction**: Resources referenced by logical identifiers rather than physical locations
2. **Portability**: Registry URIs work consistently across environments
3. **Flexibility**: Multiple addressing schemes for different needs
4. **Versioning**: Explicit support for versioning and version aliases
5. **Discoverability**: Resources organized in a structured, discoverable hierarchy
6. **Distribution**: Support for remote registries enables sharing and distribution
7. **Content Integrity**: Content-based addressing ensures resource integrity
8. **Environment Transitions**: Easily move between development, staging, and production

## Future Enhancements

1. **HTTP Registry**: Implement remote HTTP-based registries
2. **S3 Registry**: Implement S3-based registries
3. **Registry Synchronization**: Support for pushing resources between registries
4. **Authentication**: Add authentication and authorization for registry access
5. **Dependency Management**: Track and resolve dependencies between resources

## Migration

For backward compatibility, the original file-based approach is still supported. Existing manifests will continue to work without changes. To migrate to the new registry system:

1. Create a registry configuration file
2. Publish components and manifests to the registry
3. Update manifests to use registry URIs instead of file paths

This enables a gradual migration without disrupting existing workflows.
