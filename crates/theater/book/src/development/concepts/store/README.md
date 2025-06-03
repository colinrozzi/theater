# Store System

Theater's store system provides a persistent, content-addressable storage mechanism for actors and the Theater runtime. It enables efficient content deduplication, reliable data persistence, and flexible organization through a label-based reference system.

## Overview

The store is designed around a few key concepts:

1. **Content-Addressable Storage**: All data is stored using SHA-1 hashes of its content as identifiers
2. **Multiple Store Instances**: Each store has a unique UUID identifier
3. **Label System**: Friendly names can be attached to content references for easy retrieval
4. **Actor Integration**: Seamless integration with actor state management and the hash chain system
5. **Event Tracking**: Comprehensive event tracking for all store operations

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

### Label

A named reference to content in the store:

```rust
pub struct Label {
    name: String,
}
```

Labels provide:
- Human-readable names for content references
- Easy access to frequently used content
- Organizational structure for content management

### ContentStore

The core interface for interacting with a store instance:

```rust
pub struct ContentStore {
    pub id: String,
}
```

The ContentStore provides methods for:
- **Content Operations**: Store, retrieve, and check existence of content
- **Label Management**: Create, list, and remove content labels
- **Store Management**: Calculate size, list all content, etc.

## Directory Structure

The store organizes data in a UUID-based directory structure:

```
store/
├── <store-uuid1>/         # Store instance 1
│   ├── data/             # Content files stored by hash
│   │   ├── <hash1>
│   │   ├── <hash2>
│   │   └── ...
│   └── labels/           # Labels pointing to content hashes
│       ├── <label1>
│       ├── <label2>
│       └── ...
├── <store-uuid2>/         # Store instance 2
...
└── manifest/             # System metadata
```

- Each store instance has its own UUID-based directory
- Each file in the `data/` directory contains raw content, with the filename being the content's SHA-1 hash
- Each file in the `labels/` directory contains the hash of the content it references

## Usage Examples

### Creating a Store

```rust
// Create a new store with a generated UUID
let store = ContentStore::new();
println!("Created store with ID: {}", store.id());

// Create a store with a specific ID
let store = ContentStore::from_id("my-store-id");
```

### Basic Content Storage

```rust
// Store some content
let content = b"Hello, world!".to_vec();
let content_ref = store.store(content).await?;

// Retrieve the content later
let retrieved = store.get(&content_ref).await?;
assert_eq!(retrieved, b"Hello, world!".to_vec());

// Check if content exists
let exists = store.exists(&content_ref).await;
assert!(exists);
```

### Working with Labels

```rust
// Store content with a label
let content = b"Important data".to_vec();
let content_ref = store.store(content).await?;
store.label("important-data", &content_ref).await?;

// Retrieve by label
let content_ref_opt = store.get_by_label("important-data").await?;
if let Some(content_ref) = content_ref_opt {
    let retrieved = store.get(&content_ref).await?;
    println!("Retrieved labeled content: {} bytes", retrieved.len());
}

// Store and label in one operation
let content = b"New data".to_vec();
let content_ref = store.store_at_label("new-data", content).await?;

// Replace content at a label
let updated = b"Updated data".to_vec();
let new_content_ref = store.replace_content_at_label("new-data", updated).await?;

// Replace content reference at a label
store.replace_at_label("new-data", &some_existing_content_ref).await?;

// Remove a label
store.remove_label("temporary-data").await?;
```

### Content Management

```rust
// List all labels
let labels = store.list_labels().await?;
println!("Store has {} labels", labels.len());

// List all content
let contents = store.list_all_content().await?;
println!("Store has {} content items", contents.len());

// Calculate total storage size
let size = store.calculate_total_size().await?;
println!("Total storage size: {} bytes", size);
```

### Integration with Actors

Actors use the store for both state persistence and content sharing:

```rust
// In actor initialization
// Get store ID from host functions
let store_id = store::new()?;
let store = ContentStore::from_id(&store_id);

// Store actor-specific data
let data = serialize_data(&my_data)?;
let content_ref = store.store(data).await?;

// Label with actor ID for easy retrieval
store.label(format!("actor:{}", actor_id), &content_ref).await?;
```

## Store Host Functions

The store system exposes a set of host functions to WebAssembly actors through the `theater:simple/store` interface:

```rust
// Create a new store
let store_id = store::new()?;

// Store content
let content_ref = store::store(store_id, content)?;

// Get content
let content = store::get(store_id, content_ref)?;

// Check if content exists
let exists = store::exists(store_id, content_ref)?;

// Label content
store::label(store_id, "my-label", content_ref)?;

// Get content reference by label
let content_ref_opt = store::get_by_label(store_id, "my-label")?;

// Store and label in one operation
let content_ref = store::store_at_label(store_id, "my-label", content)?;

// Replace content at a label
let new_content_ref = store::replace_content_at_label(store_id, "my-label", new_content)?;

// Replace content reference at a label
store::replace_at_label(store_id, "my-label", existing_content_ref)?;

// List all labels
let labels = store::list_labels(store_id)?;

// Remove a label
store::remove_label(store_id, "my-label")?;

// Calculate total storage size
let size = store::calculate_total_size(store_id)?;

// List all content references
let content_refs = store::list_all_content(store_id)?;
```

## Event Tracking

All store operations are tracked with detailed events in the actor's state chain:

1. **Call Events**: Record when operations are called
2. **Result Events**: Record the results of successful operations
3. **Error Events**: Record any errors that occur

Each event includes:
- Store ID
- Operation type
- Content reference (when applicable)
- Label name (when applicable)
- Success/failure status
- Detailed error messages (for failures)

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