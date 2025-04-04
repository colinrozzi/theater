# Store API for Actors

This guide explains how actors can interact with the Theater store system through the host interface.

## Overview

Theater's store system provides actors with a persistent, content-addressable storage mechanism that enables:

- Storing and retrieving arbitrary data
- Sharing content between actors
- Organizing content with labels
- Building higher-level persistence patterns

## WebAssembly Interface

The store interface is exposed to WebAssembly actors through the following WIT interface:

```wit
interface store {
    /// A reference to content in the store
    record content-ref {
        hash: string,
    }

    /// Create a new store
    new: func() -> result<string, string>;

    /// Store content and return a reference
    store: func(store-id: string, content: list<u8>) -> result<content-ref, string>;

    /// Retrieve content by reference
    get: func(store-id: string, content-ref: content-ref) -> result<list<u8>, string>;

    /// Check if content exists
    exists: func(store-id: string, content-ref: content-ref) -> result<bool, string>;

    /// Label content with a string identifier
    label: func(store-id: string, label: string, content-ref: content-ref) -> result<_, string>;

    /// Get content reference by label (returns None if label doesn't exist)
    get-by-label: func(store-id: string, label: string) -> result<option<content-ref>, string>;

    /// Store content and label it in one operation
    store-at-label: func(store-id: string, label: string, content: list<u8>) -> result<content-ref, string>;

    /// Replace content at a label
    replace-content-at-label: func(store-id: string, label: string, content: list<u8>) -> result<content-ref, string>;

    /// Replace a content reference at a label
    replace-at-label: func(store-id: string, label: string, content-ref: content-ref) -> result<_, string>;

    /// Remove a label
    remove-label: func(store-id: string, label: string) -> result<_, string>;

    /// List all labels in the store
    list-labels: func(store-id: string) -> result<list<string>, string>;

    /// List all content in the store
    list-all-content: func(store-id: string) -> result<list<content-ref>, string>;

    /// Calculate total size of all content in the store
    calculate-total-size: func(store-id: string) -> result<u64, string>;
}
```

## Basic Usage

### Creating a Store

First, create a new store instance:

```rust
// Rust example
use theater::store;

// Create a new store
let store_id = store::new()?;
```

```javascript
// JavaScript example
import { store } from "theater";

// Create a new store
const storeId = await store.new();
```

### Storing and Retrieving Content

The most basic operations are storing and retrieving content:

```rust
// Rust example
use theater::store;

// Create a store
let store_id = store::new()?;

// Store some bytes
let content = b"Hello, world!".to_vec();
let content_ref = store::store(store_id.clone(), content)?;

// Retrieve the content using its reference
let retrieved = store::get(store_id.clone(), content_ref.clone())?;
assert_eq!(retrieved, b"Hello, world!".to_vec());

// Check if content exists
let exists = store::exists(store_id.clone(), content_ref.clone())?;
assert!(exists);
```

```javascript
// JavaScript example
import { store } from "theater";

// Create a store
const storeId = await store.new();

// Store some content
const content = new TextEncoder().encode("Hello, world!");
const contentRef = await store.store(storeId, content);

// Retrieve the content
const retrieved = await store.get(storeId, contentRef);
const text = new TextDecoder().decode(retrieved);
console.log(text); // "Hello, world!"

// Check if content exists
const exists = await store.exists(storeId, contentRef);
console.log(exists); // true
```

### Working with Labels

Labels provide a way to give meaningful names to content references:

```rust
// Rust example
use theater::store;

// Create a store
let store_id = store::new()?;

// Store and label in one operation
let data = serde_json::to_vec(&my_data)?;
let content_ref = store::store_at_label(store_id.clone(), "my-actor-data", data)?;

// Retrieve by label later
let content_ref_opt = store::get_by_label(store_id.clone(), "my-actor-data")?;
if let Some(content_ref) = content_ref_opt {
    let data = store::get(store_id.clone(), content_ref)?;
    let my_data: MyData = serde_json::from_slice(&data)?;
    // Use the data...
}

// Update data at a label
let updated_data = serde_json::to_vec(&new_data)?;
let new_ref = store::replace_content_at_label(store_id.clone(), "my-actor-data", updated_data)?;

// Remove a label
store::remove_label(store_id.clone(), "my-actor-data")?;
```

```javascript
// JavaScript example
import { store } from "theater";

// Create a store
const storeId = await store.new();

// Store and label data
const data = JSON.stringify({ key: "value" });
const contentRef = await store.storeAtLabel(
    storeId, 
    "my-data", 
    new TextEncoder().encode(data)
);

// Retrieve by label later
const contentRefOpt = await store.getByLabel(storeId, "my-data");
if (contentRefOpt) {
    const data = await store.get(storeId, contentRefOpt);
    const jsonData = JSON.parse(new TextDecoder().decode(data));
    console.log(jsonData.key); // "value"
}

// Update data
const updatedData = JSON.stringify({ key: "new-value" });
const newRef = await store.replaceContentAtLabel(
    storeId,
    "my-data", 
    new TextEncoder().encode(updatedData)
);
```

## Common Patterns

### Actor State Persistence

Store your actor's state outside the chain for efficiency:

```rust
// Rust example
use theater::store;

// During actor initialization
fn init() -> Result<(), String> {
    // Create a store
    let store_id = store::new()?;
    
    // Check if we have stored state
    let content_ref_opt = store::get_by_label(store_id.clone(), "my-actor-state")?;
    
    if let Some(content_ref) = content_ref_opt {
        // Restore from stored state
        let state_bytes = store::get(store_id.clone(), content_ref)?;
        let state: MyState = bincode::deserialize(&state_bytes)
            .map_err(|e| e.to_string())?;
        // Use restored state...
    } else {
        // Initialize new state
        let initial_state = MyState::default();
        let state_bytes = bincode::serialize(&initial_state)
            .map_err(|e| e.to_string())?;
        
        // Store initial state
        store::store_at_label(store_id.clone(), "my-actor-state", state_bytes)?;
    }
    
    Ok(())
}

// After state changes
fn update_stored_state(store_id: &str, state: &MyState) -> Result<(), String> {
    let state_bytes = bincode::serialize(state)
        .map_err(|e| e.to_string())?;
    
    store::replace_content_at_label(store_id.to_string(), "my-actor-state", state_bytes)?;
    Ok(())
}
```

### Versioned Content

Implement simple versioning with labeled content:

```rust
// Rust example
use theater::store;

// Store a new version
fn store_version(store_id: &str, version: u32, content: Vec<u8>) -> Result<String, String> {
    let label = format!("document:v{}", version);
    let content_ref = store::store_at_label(store_id.to_string(), &label, content)?;
    
    // Update latest pointer
    store::label(store_id.to_string(), "document:latest", &content_ref)?;
    
    Ok(content_ref.hash)
}

// Get specific version
fn get_version(store_id: &str, version: u32) -> Result<Vec<u8>, String> {
    let label = format!("document:v{}", version);
    let content_ref_opt = store::get_by_label(store_id.to_string(), &label)?;
    
    if let Some(content_ref) = content_ref_opt {
        store::get(store_id.to_string(), content_ref)
    } else {
        Err(format!("Version {} not found", version))
    }
}

// Get latest version
fn get_latest(store_id: &str) -> Result<Vec<u8>, String> {
    let content_ref_opt = store::get_by_label(store_id.to_string(), "document:latest")?;
    
    if let Some(content_ref) = content_ref_opt {
        store::get(store_id.to_string(), content_ref)
    } else {
        Err("No versions available".to_string())
    }
}
```

### Shared Resources

Share data between actors using the store:

```rust
// Actor 1: Create and share configuration
let store_id = store::new()?;
let config = Config { /* ... */ };
let config_bytes = serde_json::to_vec(&config)?;
let content_ref = store::store_at_label(store_id.clone(), "shared:config", config_bytes)?;

// Send store_id to Actor 2
send_message_to_actor("actor2", { "store_id": store_id });

// Actor 2: Access shared configuration
fn handle_message(message) {
    let store_id = message.store_id;
    let content_ref_opt = store::get_by_label(store_id, "shared:config")?;
    
    if let Some(content_ref) = content_ref_opt {
        let config_bytes = store::get(store_id, content_ref)?;
        let config: Config = serde_json::from_slice(&config_bytes)?;
        // Use the shared config...
    }
}
```

## Best Practices

### Content Organization

Develop a consistent labeling scheme:

1. **Namespaced Labels**: Use prefixes to group related content
   - `actor:{id}:state` - Actor-specific state
   - `shared:{resource}` - Resources shared between actors
   - `config:{component}` - Configuration data

2. **Label Granularity**: Balance between too many and too few labels
   - Too many: Management overhead
   - Too few: Loss of organization

### Efficient Content Storage

Optimize your use of the store:

1. **Content Size**: The store works best with small to medium data (< 10MB)
   - For larger data, consider splitting into chunks
   
2. **Deduplication**: Take advantage of content-addressing
   - Same content is stored only once
   - Use references to point to the same content

3. **Batching**: Minimize store operations by batching changes
   - Accumulate changes before storing

### Error Handling

Always handle store errors appropriately:

```rust
match store::get_by_label(store_id.clone(), "my-label") {
    Ok(content_ref_opt) => {
        if let Some(content_ref) = content_ref_opt {
            // Process content...
        } else {
            // Handle missing label content
        }
    },
    Err(e) => {
        // Handle store error
        eprintln!("Store error: {}", e);
    }
}
```

## Advanced Use Cases

### Content-Based Addressing

Implement data structures using content references:

```rust
// Content-addressed tree
struct TreeNode {
    value: String,
    left_ref: Option<String>,  // ContentRef hash
    right_ref: Option<String>, // ContentRef hash
}

// Store a node
fn store_node(store_id: &str, node: &TreeNode) -> Result<String, String> {
    let bytes = serde_json::to_vec(node).map_err(|e| e.to_string())?;
    let content_ref = store::store(store_id.to_string(), bytes)?;
    Ok(content_ref.hash)
}

// Load a node
fn load_node(store_id: &str, hash: &str) -> Result<TreeNode, String> {
    let content_ref = ContentRef { hash: hash.to_string() };
    let bytes = store::get(store_id.to_string(), content_ref)?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

// Create a simple tree
let store_id = store::new()?;

let leaf1 = TreeNode { 
    value: "Leaf 1".to_string(), 
    left_ref: None, 
    right_ref: None 
};
let leaf2 = TreeNode { 
    value: "Leaf 2".to_string(), 
    left_ref: None, 
    right_ref: None 
};

let leaf1_ref = store_node(&store_id, &leaf1)?;
let leaf2_ref = store_node(&store_id, &leaf2)?;

let root = TreeNode {
    value: "Root".to_string(),
    left_ref: Some(leaf1_ref),
    right_ref: Some(leaf2_ref),
};

let root_ref = store_node(&store_id, &root)?;
let content_ref = ContentRef { hash: root_ref };
store::label(store_id.clone(), "tree:root", content_ref)?;
```

### Cached Computations

Cache expensive computation results:

```rust
// Create a store to use for caching
let store_id = store::new()?;

// Check if we have cached result
let cache_label = format!("cache:computation:{}", input_hash);
let content_ref_opt = store::get_by_label(store_id.clone(), &cache_label)?;

if let Some(content_ref) = content_ref_opt {
    // Use cached result
    let result_bytes = store::get(store_id.clone(), content_ref)?;
    let result: ComputationResult = deserialize(&result_bytes)?;
    return Ok(result);
}

// Perform expensive computation
let result = expensive_computation(input)?;

// Cache the result
let result_bytes = serialize(&result)?;
store::store_at_label(store_id.clone(), &cache_label, result_bytes)?;

Ok(result)
```

## Limitations and Considerations

1. **No Transactions**: Operations are individual and cannot be rolled back
2. **No Query System**: Content can only be retrieved by explicit reference or label
3. **Performance**: Consider the overhead of store operations in performance-critical code
4. **Memory Usage**: Be mindful of data size to avoid excessive memory consumption

## Summary

The store API provides actors with powerful persistence capabilities while maintaining the integrity and security of the Theater system. By following these patterns and best practices, you can effectively leverage the store for your actor's data management needs.