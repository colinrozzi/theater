# Theater Store Handler

Content storage handler for the Theater WebAssembly runtime.

## Overview

The Store Handler provides content-addressed storage capabilities to WebAssembly actors in the Theater system. It enables actors to store, retrieve, and manage content with labels in a secure and auditable manner.

## Features

- **Content-Addressed Storage**: All content is stored using SHA1 hashing for integrity
- **Label Management**: Organize content with human-readable labels
- **Full Auditability**: All operations are recorded in the event chain
- **Permission Control**: Configurable permissions for store access
- **13 Storage Operations**: Complete API for content management

## Operations

### Core Operations

- `new()` - Create a new content store instance
- `store(content)` - Store content and get a content reference
- `get(content_ref)` - Retrieve content by reference
- `exists(content_ref)` - Check if content exists

### Label Operations

- `label(label, content_ref)` - Add a label to existing content
- `get-by-label(label)` - Get content reference by label
- `remove-label(label)` - Remove a label
- `store-at-label(label, content)` - Store content and immediately label it
- `replace-content-at-label(label, content)` - Replace content at a label
- `replace-at-label(label, content_ref)` - Replace reference at a label

### Utility Operations

- `list-all-content()` - List all content references
- `calculate-total-size()` - Calculate total size of all stored content
- `list-labels()` - List all labels

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
theater-handler-store = "0.2"
```

### Basic Example

```rust
use theater_handler_store::StoreHandler;
use theater::config::actor_manifest::StoreHandlerConfig;
use theater::handler::Handler;

// Create the handler
let config = StoreHandlerConfig {};
let handler = StoreHandler::new(config, None);

// The handler can now be registered with the Theater runtime
```

### In Actor Manifests

```toml
[[handlers]]
type = "store"
```

## Architecture

The Store Handler implements the Theater `Handler` trait and provides:

- Synchronous setup of host functions
- Async operations for all storage operations
- Complete event chain recording for auditability
- Integration with the Theater content storage system

All storage operations are recorded in the actor's event chain, providing a complete audit trail of all content operations.

## Event Recording

Every operation records events including:

- **Setup Events**: Handler initialization and configuration
- **Operation Events**: Each store/retrieve/label operation
- **Result Events**: Success/failure of operations with details
- **Error Events**: Detailed error information for debugging

## Security

The Store Handler integrates with Theater's permission system:

- Content is isolated per store instance
- All operations are auditable through event chains
- No direct filesystem access from actors
- Content-addressed storage prevents tampering

## Migration from Core Theater

This handler was migrated from the core `theater` crate (`src/host/store.rs`) to provide:

- ✅ Better modularity
- ✅ Independent testing
- ✅ Clearer architecture
- ✅ Simplified dependencies

## Development

Run tests:

```bash
cargo test -p theater-handler-store
```

## License

See the LICENSE file in the repository root.
