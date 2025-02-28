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
    type content-ref = string;

    /// Store content and return a reference
    store: func(content: list<u8>) -> result<content-ref, string>;

    /// Retrieve content by reference
    get: func(content-ref: content-ref) -> result<list<u8>, string>;

    /// Check if content exists
    exists: func(content-ref: content-ref) -> result<bool, string>;

    /// Label content with a string identifier
    label: func(label: string, content-ref: content-ref) -> result<_, string>;

    /// Get content references by label
    get-by-label: func(label: string) -> result<list<content-ref>, string>;

    /// Store content and label it in one operation
    put-at-label: func(label: string, content: list<u8>) -> result<content-ref, string>;

    /// Replace content at a label
    replace-at-label: func(label: string, content: list<u8>) -> result<content-ref, string>;

    /// List all labels
    list-labels: func() -> result<list<string>, string>;
}
```

## Basic Usage

### Storing and Retrieving Content

The most basic operations are storing and retrieving content:

```rust
// Rust example
use theater::store;

// Store some bytes
let content = b"Hello, world!".to_vec();
let content_ref = store::store(content)?;

// Retrieve the content using its reference
let retrieved = store::get(&content_ref)?;
assert_eq!(retrieved, b"Hello, world!".to_vec());

// Check if content exists
let exists = store::exists(&content_ref)?;
assert!(exists);
```

```javascript
// JavaScript example
import { store } from "theater";

// Store some content
const content = new TextEncoder().encode("Hello, world!");
const contentRef = await store.store(content);

// Retrieve the content
const retrieved = await store.get(contentRef);
const text = new TextDecoder().decode(retrieved);
console.log(text); // "Hello, world!"

// Check if content exists
const exists = await store.exists(contentRef);
console.log(exists); // true
```

### Working with Labels

Labels provide a way to give meaningful names to content references:

```rust
// Rust example
use theater::store;

// Store and label in one operation
let data = serde_json::to_vec(&my_data)?;
let content_ref = store::put_at_label("my-actor-data", data)?;

// Retrieve by label later
let refs = store::get_by_label("my-actor-data")?;
if let Some(ref_id) = refs.first() {
    let data = store::get(ref_id)?;
    let my_data: MyData = serde_json::from_slice(&data)?;
    // Use the data...
}

// Update data at a label
let updated_data = serde_json::to_vec(&new_data)?;
let new_ref = store::replace_at_label("my-actor-data", updated_data)?;
```

```javascript
// JavaScript example
import { store } from "theater";

// Store and label data
const data = JSON.stringify({ key: "value" });
const contentRef = await store.putAtLabel("my-data", 
    new TextEncoder().encode(data));

// Retrieve by label later
const refs = await store.getByLabel("my-data");
if (refs.length > 0) {
    const data = await store.get(refs[0]);
    const jsonData = JSON.parse(new TextDecoder().decode(data));
    console.log(jsonData.key); // "value"
}

// Update data
const updatedData = JSON.stringify({ key: "new-value" });
const newRef = await store.replaceAtLabel("my-data", 
    new TextEncoder().encode(updatedData));
```

## Common Patterns

### Actor State Persistence

Store your actor's state outside the chain for efficiency:

```rust
// Rust example
use theater::store;

// During actor initialization
fn init() -> Result<(), String> {
    // Check if we have stored state
    let refs = store::get_by_label("my-actor-state")?;
    
    if let Some(ref_id) = refs.first() {
        // Restore from stored state
        let state_bytes = store::get(ref_id)?;
        let state: MyState = bincode::deserialize(&state_bytes)
            .map_err(|e| e.to_string())?;
        // Use restored state...
    } else {
        // Initialize new state
        let initial_state = MyState::default();
        let state_bytes = bincode::serialize(&initial_state)
            .map_err(|e| e.to_string())?;
        
        // Store initial state
        store::put_at_label("my-actor-state", state_bytes)?;
    }
    
    Ok(())
}

// After state changes
fn update_stored_state(state: &MyState) -> Result<(), String> {
    let state_bytes = bincode::serialize(state)
        .map_err(|e| e.to_string())?;
    
    store::replace_at_label("my-actor-state", state_bytes)?;
    Ok(())
}
```

### Versioned Content

Implement simple versioning with labeled content:

```rust
// Rust example
use theater::store;

// Store a new version
fn store_version(version: u32, content: Vec<u8>) -> Result<String, String> {
    let label = format!("document:v{}", version);
    let content_ref = store::put_at_label(&label, content)?;
    
    // Update latest pointer
    store::label("document:latest", &content_ref)?;
    
    Ok(content_ref)
}

// Get specific version
fn get_version(version: u32) -> Result<Vec<u8>, String> {
    let label = format!("document:v{}", version);
    let refs = store::get_by_label(&label)?;
    
    if let Some(ref_id) = refs.first() {
        store::get(ref_id)
    } else {
        Err(format!("Version {} not found", version))
    }
}

// Get latest version
fn get_latest() -> Result<Vec<u8>, String> {
    let refs = store::get_by_label("document:latest")?;
    
    if let Some(ref_id) = refs.first() {
        store::get(ref_id)
    } else {
        Err("No versions available".to_string())
    }
}
```

### Shared Resources

Share data between actors using the store:

```rust
// Actor 1: Create and share configuration
let config = Config { /* ... */ };
let config_bytes = serde_json::to_vec(&config)?;
let content_ref = store::put_at_label("shared:config", config_bytes)?;

// Actor 2: Access shared configuration
let refs = store::get_by_label("shared:config")?;
if let Some(ref_id) = refs.first() {
    let config_bytes = store::get(ref_id)?;
    let config: Config = serde_json::from_slice(&config_bytes)?;
    // Use the shared config...
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
match store::get_by_label("my-label") {
    Ok(refs) => {
        if let Some(ref_id) = refs.first() {
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
    left_ref: Option<String>,  // ContentRef as String
    right_ref: Option<String>, // ContentRef as String
}

// Store a node
fn store_node(node: &TreeNode) -> Result<String, String> {
    let bytes = serde_json::to_vec(node).map_err(|e| e.to_string())?;
    store::store(bytes)
}

// Load a node
fn load_node(ref_id: &str) -> Result<TreeNode, String> {
    let bytes = store::get(ref_id)?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

// Create a simple tree
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

let leaf1_ref = store_node(&leaf1)?;
let leaf2_ref = store_node(&leaf2)?;

let root = TreeNode {
    value: "Root".to_string(),
    left_ref: Some(leaf1_ref),
    right_ref: Some(leaf2_ref),
};

let root_ref = store_node(&root)?;
store::label("tree:root", &root_ref)?;
```

### Cached Computations

Cache expensive computation results:

```rust
// Check if we have cached result
let cache_label = format!("cache:computation:{}", input_hash);
let refs = store::get_by_label(&cache_label)?;

if let Some(ref_id) = refs.first() {
    // Use cached result
    let result_bytes = store::get(ref_id)?;
    let result: ComputationResult = deserialize(&result_bytes)?;
    return Ok(result);
}

// Perform expensive computation
let result = expensive_computation(input)?;

// Cache the result
let result_bytes = serialize(&result)?;
store::put_at_label(&cache_label, result_bytes)?;

Ok(result)
```

## Limitations and Considerations

1. **No Transactions**: Operations are individual and cannot be rolled back
2. **No Query System**: Content can only be retrieved by explicit reference or label
3. **Performance**: Consider the overhead of store operations in performance-critical code
4. **Memory Usage**: Be mindful of data size to avoid excessive memory consumption

## Summary

The store API provides actors with powerful persistence capabilities while maintaining the integrity and security of the Theater system. By following these patterns and best practices, you can effectively leverage the store for your actor's data management needs.
