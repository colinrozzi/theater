# Store Handler

The Store Handler provides actors with access to Theater's content-addressable storage system. It enables actors to store and retrieve data using content hashes, create and manage labels for easier reference, and maintain persistent data across actor restarts.

## Overview

The Store Handler implements the `theater:simple/store` interface, enabling actors to:

1. Create and manage store instances
2. Store and retrieve data using content-addressable storage
3. Create and manage labels for easy content reference
4. Check for content existence and calculate storage size
5. Efficiently deduplicate content
6. Persistently store data across actor restarts and system reboots

## Configuration

To use the Store Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "store"
config = {}
```

The Store Handler doesn't currently require any specific configuration parameters.

## Interface

The Store Handler is defined using the following WIT interface:

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

## Store Management Operations

### Creating a Store

To create a new store instance:

```rust
match store::new() {
    Ok(store_id) => {
        println!("Created new store with ID: {}", store_id);
        // Save the store ID for future operations
    },
    Err(error) => {
        println!("Failed to create store: {}", error);
    }
}
```

## Content Storage Operations

### Storing Content

To store content in the store:

```rust
let data = b"Important data".to_vec();

match store::store(store_id.clone(), data) {
    Ok(content_ref) => {
        println!("Content stored with hash: {}", content_ref.hash);
        // Save the content reference for future use
    },
    Err(error) => {
        println!("Failed to store content: {}", error);
    }
}
```

### Retrieving Content

To retrieve content using a content reference:

```rust
match store::get(store_id.clone(), content_ref) {
    Ok(content) => {
        // Process the retrieved content
        let text = String::from_utf8(content).expect("Not valid UTF-8");
        println!("Retrieved content: {}", text);
    },
    Err(error) => {
        println!("Failed to retrieve content: {}", error);
    }
}
```

### Checking Content Existence

To check if content exists in the store:

```rust
match store::exists(store_id.clone(), content_ref) {
    Ok(exists) => {
        if exists {
            println!("Content exists in the store");
        } else {
            println!("Content does not exist in the store");
        }
    },
    Err(error) => {
        println!("Failed to check content existence: {}", error);
    }
}
```

## Label Operations

Labels provide a way to assign human-readable names to content references, making it easier to retrieve them later.

### Creating Labels

To create a label for content:

```rust
match store::label(store_id.clone(), "important-data", content_ref) {
    Ok(_) => {
        println!("Label 'important-data' created successfully");
    },
    Err(error) => {
        println!("Failed to create label: {}", error);
    }
}
```

### Getting Content by Label

To retrieve content reference using a label:

```rust
match store::get_by_label(store_id.clone(), "important-data") {
    Ok(content_ref_opt) => {
        if let Some(content_ref) = content_ref_opt {
            // Use the content reference to get the actual content
            let content = store::get(store_id.clone(), content_ref)?;
            println!("Retrieved content for label 'important-data'");
        } else {
            println!("Label 'important-data' does not exist");
        }
    },
    Err(error) => {
        println!("Failed to get content by label: {}", error);
    }
}
```

### Storing and Labeling in One Operation

To store content and create a label in one operation:

```rust
let data = b"New data".to_vec();

match store::store_at_label(store_id.clone(), "new-data", data) {
    Ok(content_ref) => {
        println!("Content stored and labeled as 'new-data'");
    },
    Err(error) => {
        println!("Failed to store and label content: {}", error);
    }
}
```

### Replacing Content at a Label

To replace the content referenced by a label:

```rust
let updated_data = b"Updated data".to_vec();

match store::replace_content_at_label(store_id.clone(), "new-data", updated_data) {
    Ok(content_ref) => {
        println!("Content at label 'new-data' updated successfully");
    },
    Err(error) => {
        println!("Failed to update content: {}", error);
    }
}
```

### Replacing a Content Reference at a Label

To replace the content reference at a label with another existing reference:

```rust
match store::replace_at_label(store_id.clone(), "new-data", existing_content_ref) {
    Ok(_) => {
        println!("Content reference at label 'new-data' replaced successfully");
    },
    Err(error) => {
        println!("Failed to replace content reference: {}", error);
    }
}
```

### Listing Labels

To get a list of all labels:

```rust
match store::list_labels(store_id.clone()) {
    Ok(labels) => {
        println!("Available labels:");
        for label in labels {
            println!("- {}", label);
        }
    },
    Err(error) => {
        println!("Failed to list labels: {}", error);
    }
}
```

### Removing Labels

To remove a label:

```rust
match store::remove_label(store_id.clone(), "temporary-data") {
    Ok(_) => {
        println!("Label 'temporary-data' removed successfully");
    },
    Err(error) => {
        println!("Failed to remove label: {}", error);
    }
}
```

## Store Management

### Calculating Total Size

To calculate the total size of all stored content:

```rust
match store::calculate_total_size(store_id.clone()) {
    Ok(size) => {
        println!("Total storage size: {} bytes", size);
    },
    Err(error) => {
        println!("Failed to calculate storage size: {}", error);
    }
}
```

### Listing All Content

To list all content references in the store:

```rust
match store::list_all_content(store_id.clone()) {
    Ok(refs) => {
        println!("Total content items: {}", refs.len());
        for content_ref in refs {
            println!("- {}", content_ref.hash);
        }
    },
    Err(error) => {
        println!("Failed to list content: {}", error);
    }
}
```

## Label Naming Conventions

While you can use any string as a label, it's good practice to follow certain conventions:

1. **Actor-Specific Labels**: Prefix labels with the actor ID or name
   ```
   actor:12345:config
   ```

2. **Versioned Labels**: Include version information in labels
   ```
   config:v1.0
   ```

3. **Type Labels**: Include content type in the label
   ```
   image:logo
   ```

4. **Namespaced Labels**: Use namespaces for organization
   ```
   app:settings:theme
   ```

## State Chain Integration

Store operations are recorded in the actor's state chain, ensuring a verifiable history of all storage interactions. The chain events include:

### Call Events
- `NewStoreCall`
- `StoreCall`
- `GetCall`
- `ExistsCall`
- `LabelCall`
- `GetByLabelCall`
- `StoreAtLabelCall`
- `ReplaceContentAtLabelCall`
- `ReplaceAtLabelCall`
- `RemoveLabelCall`
- `ListLabelsCall`
- `ListAllContentCall`
- `CalculateTotalSizeCall`

### Result Events
- `NewStoreResult`
- `StoreResult`
- `GetResult`
- `ExistsResult`
- `LabelResult`
- `GetByLabelResult`
- `StoreAtLabelResult`
- `ReplaceContentAtLabelResult`
- `ReplaceAtLabelResult`
- `RemoveLabelResult`
- `ListLabelsResult`
- `ListAllContentResult`
- `CalculateTotalSizeResult`

### Error Events
- `Error` (includes operation type and error message)

Each event includes detailed information such as store ID, content references, labels, and success/failure status.

## Error Handling

The Store Handler provides detailed error information for various failure scenarios:

1. **Storage Errors**: When content storage fails
2. **Retrieval Errors**: When content retrieval fails
3. **Label Errors**: When label operations fail
4. **Not Found Errors**: When content or labels don't exist
5. **IO Errors**: When disk operations fail

## Security Considerations

When using the Store Handler, consider the following security aspects:

1. **Content Validation**: Validate data before storing it
2. **Label Namespaces**: Use namespaced labels to avoid conflicts
3. **Size Limits**: Be mindful of storage size and implement limits
4. **Sensitive Data**: Consider encrypting sensitive data before storage
5. **Cleanup**: Implement policies for removing unused content

## Implementation Details

Under the hood, the Store Handler:

1. Uses SHA-1 hashing to create unique content identifiers
2. Stores content in a directory structure organized by store ID
3. Maintains separate directories for content and label mappings
4. Records detailed events for all operations
5. Ensures data integrity through content verification

## Storage Structure

The physical storage is organized as follows:

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

## Best Practices

1. **Store Management**: Create separate stores for different use cases
2. **Content Size**: The store is optimized for small to medium content sizes (< 10MB)
3. **Reference Tracking**: Keep track of content references for important data
4. **Label Schemes**: Develop consistent label naming schemes
5. **Cleanup**: Implement periodic cleanup for unused content
6. **Error Handling**: Always handle store operation errors appropriately
7. **Caching**: Consider implementing local caching for frequently accessed content

## Common Use Cases

### Configuration Storage

```rust
// Store configuration
fn save_config(store_id: &str, config: &Config) -> Result<(), String> {
    let config_bytes = serde_json::to_vec(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    store::store_at_label(store_id.to_string(), "app:config", config_bytes)
        .map(|_| ())
        .map_err(|e| format!("Failed to store config: {}", e))
}

// Load configuration
fn load_config(store_id: &str) -> Result<Config, String> {
    let content_ref_opt = store::get_by_label(store_id.to_string(), "app:config")
        .map_err(|e| format!("Failed to get config reference: {}", e))?;
    
    if let Some(content_ref) = content_ref_opt {
        let config_bytes = store::get(store_id.to_string(), content_ref)
            .map_err(|e| format!("Failed to retrieve config: {}", e))?;
        
        let config: Config = serde_json::from_slice(&config_bytes)
            .map_err(|e| format!("Failed to deserialize config: {}", e))?;
        
        Ok(config)
    } else {
        Err("Configuration not found".to_string())
    }
}
```

### Content Deduplication

```rust
fn store_with_deduplication(store_id: &str, data: Vec<u8>) -> Result<ContentRef, String> {
    // Generate a hash to check if the content already exists
    use sha1::{Sha1, Digest};
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let hash = format!("{:x}", hasher.finalize());
    
    // Create a content reference to check existence
    let content_ref = ContentRef { hash };
    
    // Check if the content already exists
    if store::exists(store_id.to_string(), content_ref.clone())? {
        println!("Content already exists in store, reusing existing reference");
        return Ok(content_ref);
    }
    
    // Content doesn't exist, store it
    store::store(store_id.to_string(), data)
}
```

### Versioned Content

```rust
fn store_versioned_content(store_id: &str, name: &str, version: &str, data: Vec<u8>) -> Result<(), String> {
    // Store the content
    let content_ref = store::store(store_id.to_string(), data)?;
    
    // Create a versioned label
    let versioned_label = format!("{}:v{}", name, version);
    store::label(store_id.to_string(), versioned_label, content_ref.clone())?;
    
    // Always update the 'latest' label
    let latest_label = format!("{}:latest", name);
    store::label(store_id.to_string(), latest_label, content_ref)?;
    
    Ok(())
}

fn get_content_version(store_id: &str, name: &str, version: &str) -> Result<Vec<u8>, String> {
    let label = format!("{}:v{}", name, version);
    let content_ref_opt = store::get_by_label(store_id.to_string(), label)?;
    
    if let Some(content_ref) = content_ref_opt {
        store::get(store_id.to_string(), content_ref)
    } else {
        Err(format!("Version {} not found", version))
    }
}

fn get_latest_content(store_id: &str, name: &str) -> Result<Vec<u8>, String> {
    let label = format!("{}:latest", name);
    let content_ref_opt = store::get_by_label(store_id.to_string(), label)?;
    
    if let Some(content_ref) = content_ref_opt {
        store::get(store_id.to_string(), content_ref)
    } else {
        Err(format!("No versions available for {}", name))
    }
}
```

## Related Topics

- [Filesystem Handler](filesystem.md) - Alternative file access mechanism
- [State Management](../core-concepts/state-management.md) - For understanding state chain integration
- [Store System](../core-concepts/store/README.md) - For deeper store concepts
- [Store API for Actors](../core-concepts/store/actor-api.md) - For actor-specific store usage
- [Store Usage Patterns](../core-concepts/store/usage-patterns.md) - For common usage patterns and examples

## Event Types Reference

The Store Handler tracks detailed events for all operations. Here's a complete reference of the event types:

### Call Events

| Event Type | Description | Parameters |
|------------|-------------|------------|
| `NewStoreCall` | Called when creating a new store | None |
| `StoreCall` | Called when storing content | `store_id`, `content` |
| `GetCall` | Called when retrieving content | `store_id`, `content_ref` |
| `ExistsCall` | Called when checking if content exists | `store_id`, `content_ref` |
| `LabelCall` | Called when labeling content | `store_id`, `label`, `content_ref` |
| `GetByLabelCall` | Called when getting content by label | `store_id`, `label` |
| `StoreAtLabelCall` | Called when storing and labeling content | `store_id`, `label`, `content` |
| `ReplaceContentAtLabelCall` | Called when replacing content at a label | `store_id`, `label`, `content` |
| `ReplaceAtLabelCall` | Called when replacing a reference at a label | `store_id`, `label`, `content_ref` |
| `RemoveLabelCall` | Called when removing a label | `store_id`, `label` |
| `ListLabelsCall` | Called when listing all labels | `store_id` |
| `ListAllContentCall` | Called when listing all content | `store_id` |
| `CalculateTotalSizeCall` | Called when calculating total size | `store_id` |

### Result Events

| Event Type | Description | Parameters |
|------------|-------------|------------|
| `NewStoreResult` | Result of creating a new store | `store_id`, `success` |
| `StoreResult` | Result of storing content | `store_id`, `content_ref`, `success` |
| `GetResult` | Result of retrieving content | `store_id`, `content_ref`, `content`, `success` |
| `ExistsResult` | Result of checking if content exists | `store_id`, `content_ref`, `exists`, `success` |
| `LabelResult` | Result of labeling content | `store_id`, `label`, `content_ref`, `success` |
| `GetByLabelResult` | Result of getting content by label | `store_id`, `label`, `content_ref`, `success` |
| `StoreAtLabelResult` | Result of storing and labeling content | `store_id`, `label`, `content_ref`, `success` |
| `ReplaceContentAtLabelResult` | Result of replacing content at a label | `store_id`, `label`, `content_ref`, `success` |
| `ReplaceAtLabelResult` | Result of replacing a reference at a label | `store_id`, `label`, `content_ref`, `success` |
| `RemoveLabelResult` | Result of removing a label | `store_id`, `label`, `success` |
| `ListLabelsResult` | Result of listing all labels | `store_id`, `labels`, `success` |
| `ListAllContentResult` | Result of listing all content | `store_id`, `content_refs`, `success` |
| `CalculateTotalSizeResult` | Result of calculating total size | `store_id`, `size`, `success` |

### Error Events

| Event Type | Description | Parameters |
|------------|-------------|------------|
| `Error` | Records an error with any operation | `operation`, `message` |

Each event includes a timestamp and optional description field in addition to the operation-specific parameters.