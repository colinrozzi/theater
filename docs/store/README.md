# Store System

Theater's store system provides a persistent, content-addressable storage mechanism for actors and the Theater runtime. It enables efficient content deduplication, reliable data persistence, and flexible organization through a label-based reference system.

## Overview

The store is designed around a few key concepts:

1. **Content-Addressable Storage**: All data is stored using SHA-1 hashes of its content as identifiers
2. **Concurrency-Safe Operations**: Store operations run in their own thread with message-passing interface
3. **Label System**: Friendly names can be attached to content references for easy retrieval
4. **Actor Integration**: Seamless integration with actor state management and the hash chain system

## Core Components

### ContentRef

A reference to content in the store, identified by its SHA-1 hash:

```rust
pub struct ContentRef {
    hash: String,
}
```

ContentRefs are used to:
- Uniquely identify content without duplication
- Reference content across system components
- Track content through labels and other organizational structures

### ContentStore

The client interface for interacting with the store. It provides methods for:

- **Content Operations**: Store, retrieve, and check existence of content
- **Label Management**: Create, list, and remove content labels
- **Store Management**: Calculate size, list all content, etc.

The `ContentStore` runs as a separate task, communicating via message passing to ensure thread safety.

## Directory Structure

The store organizes data in a simple directory structure:

```
store/
├── data/         # Content files stored by hash
│   ├── 0044950809b9af88e64d8d0f809e24936ca96ebc
│   ├── 00746f46516b19261181d0b9c7f69764d8f3d307
│   └── ...
└── labels/       # Labels pointing to content hashes
    ├── 07e5472c-fdb9-43e2-a93d-33f5fdc8cbb2:chain-head
    ├── 162ffc76-15c4-4d2a-8c31-3999a816f8be:chain-head
    └── ...
```

- Each file in `data/` contains the raw content, with the filename being the content's SHA-1 hash
- Each file in `labels/` contains one or more hash references, allowing lookup by label name

## Usage Examples

### Basic Content Storage

```rust
// Store some content
let content = b"Hello, world!".to_vec();
let content_ref = store.store(content).await?;

// Retrieve the content later
let retrieved = store.get(content_ref.clone()).await?;
assert_eq!(retrieved, b"Hello, world!".to_vec());

// Check if content exists
let exists = store.exists(content_ref).await?;
assert!(exists);
```

### Working with Labels

```rust
// Store content with a label
let content = b"Important data".to_vec();
let content_ref = store.store(content).await?;
store.label("important-data".to_string(), content_ref.clone()).await?;

// Retrieve by label
let refs = store.get_by_label("important-data".to_string()).await?;
let retrieved = store.get(refs[0].clone()).await?;

// Store and label in one operation
let content = b"New data".to_vec();
let content_ref = store.put_at_label("new-data".to_string(), content).await?;

// Replace content at a label
let updated = b"Updated data".to_vec();
store.replace_content_at_label("new-data".to_string(), updated).await?;
```

### Integration with Actors

Actors use the store for both state persistence and content sharing:

```rust
// In actor initialization
let store = actor_store.content_store.clone();

// Store actor-specific data
let data = serialize_data(&my_data)?;
let content_ref = store.store(data).await?;

// Label with actor ID for easy retrieval
store.label(format!("actor:{}", actor_id), content_ref).await?;
```

## State Chain Integration

The store system is tightly integrated with Theater's state chain mechanism:

1. **Chain Persistence**: State chains are stored using content references
2. **Chain Head Tracking**: The latest state in a chain is tracked via labels
3. **Event Storage**: Individual chain events are stored with content references

### Chain Events and Labels

Chain heads are tracked with specific label patterns:

```
{actor-id}:chain-head
```

This allows for efficient retrieval of the current state of any actor by its ID.

## Advanced Features

### Content Resolution

The store provides a flexible resolution system for content references:

```rust
// Different ways to refer to content
let content1 = store.resolve_reference("store:my-label").await?;
let content2 = store.resolve_reference("store:hash:a1b2c3...").await?;
let content3 = store.resolve_reference("/path/to/file").await?;
```

This allows for a unified interface to access content from different sources.

### Store Management

Operations for managing the store itself:

```rust
// List all labels
let labels = store.list_labels().await?;

// Calculate total storage size
let size_bytes = store.calculate_total_size().await?;

// List all content references
let all_refs = store.list_all_content().await?;
```

## Concurrency and Safety

The store implements a thread-safe design using Tokio's message passing:

1. All operations are sent as messages to a dedicated store thread
2. Responses are returned via oneshot channels
3. The store implementation handles concurrency internally

This design ensures that:
- Multiple actors can safely interact with the store concurrently
- Long-running operations don't block the actor system
- Store operations are properly sequenced

## Configuration

The store system supports configuration through:

1. **Base Path**: The root directory for the store
2. **THEATER_HOME**: Environment variable for locating the store

Example configuration:

```rust
// Create store with explicit path
let store = ContentStore::start(PathBuf::from("/path/to/store"));

// Using THEATER_HOME environment variable
// THEATER_HOME=/home/user/theater
let store = ContentStore::start(PathBuf::from("store"));
// This will use /home/user/theater/store
```

## Best Practices

1. **Content Size**: Store is optimized for small to medium content sizes (< 10MB)
2. **Reference Tracking**: Keep track of content references for important data
3. **Label Schemes**: Develop consistent label naming schemes for your application
4. **Cleanup**: Implement periodic cleanup for unused content
5. **Error Handling**: Always handle store operation errors appropriately

## Limitations and Future Enhancements

Current limitations:
- No automatic garbage collection of unused content
- Limited indexing capabilities
- No built-in encryption for stored content

Planned enhancements:
- Smarter content pruning mechanisms
- More advanced querying capabilities
- Content encryption options
- Cross-node content synchronization
