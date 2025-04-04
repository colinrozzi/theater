# Store Handler

The Store Handler provides actors with access to Theater's content-addressable storage system. It enables actors to store and retrieve data using content hashes, create and manage labels for easier reference, and maintain persistent data across actor restarts.

## Overview

The Store Handler implements the `ntwk:theater/store` interface, enabling actors to:

1. Store and retrieve data using content-addressable storage
2. Create and manage labels for easy content reference
3. Check for content existence and calculate storage size
4. Efficiently deduplicate content
5. Persistently store data across actor restarts and system reboots

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
    // Store content and get a content reference
    store: func(content: list<u8>) -> result<content-ref, string>;
    
    // Get content by reference
    get: func(ref: content-ref) -> result<list<u8>, string>;
    
    // Check if content exists
    exists: func(ref: content-ref) -> result<bool, string>;
    
    // Create or update a label for content
    label: func(label-name: string, ref: content-ref) -> result<_, string>;
    
    // Get content reference by label
    get-by-label: func(label-name: string) -> result<content-ref, string>;
    
    // Store content and label it in one operation
    put-at-label: func(label-name: string, content: list<u8>) -> result<content-ref, string>;
    
    // Replace content at an existing label
    replace-content-at-label: func(label-name: string, content: list<u8>) -> result<content-ref, string>;
    
    // List all labels
    list-labels: func() -> result<list<string>, string>;
    
    // Remove a label
    remove-label: func(label-name: string) -> result<_, string>;
    
    // Calculate total storage size
    calculate-total-size: func() -> result<u64, string>;
    
    // List all content references
    list-all-content: func() -> result<list<content-ref>, string>;
    
    // Content reference record
    record content-ref {
        hash: string,
    }
}
```

## Content Storage Operations

### Storing Content

To store content in the store:

```rust
let data = b"Important data".to_vec();

match store::store(data) {
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
match store::get(content_ref) {
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
match store::exists(content_ref) {
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
match store::label("important-data".to_string(), content_ref) {
    Ok(_) => {
        println!("Label 'important-data' created successfully");
    },
    Err(error) => {
        println!("Failed to create label: {}", error);
    }
}
```

### Getting Content by Label

To retrieve content using a label:

```rust
match store::get_by_label("important-data".to_string()) {
    Ok(content_ref) => {
        // Use the content reference to get the actual content
        let content = store::get(content_ref)?;
        println!("Retrieved content for label 'important-data'");
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

match store::put_at_label("new-data".to_string(), data) {
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

match store::replace_content_at_label("new-data".to_string(), updated_data) {
    Ok(content_ref) => {
        println!("Content at label 'new-data' updated successfully");
    },
    Err(error) => {
        println!("Failed to update content: {}", error);
    }
}
```

### Listing Labels

To get a list of all labels:

```rust
match store::list_labels() {
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
match store::remove_label("temporary-data".to_string()) {
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
match store::calculate_total_size() {
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
match store::list_all_content() {
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

1. **StoreOperation**: Records details of storage operations:
   - Operation type (store, get, label, etc.)
   - Content reference hash (when applicable)
   - Label name (when applicable)
   - Content size (for store operations)

2. **Error**: Records any errors that occur:
   - Operation type
   - Error message

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
2. Stores content in a simple directory structure
3. Maintains a separate directory for label mappings
4. Handles concurrency through message passing
5. Ensures data integrity through content verification

## Storage Structure

The physical storage is organized as follows:

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

## Best Practices

1. **Content Size**: The store is optimized for small to medium content sizes (< 10MB)
2. **Reference Tracking**: Keep track of content references for important data
3. **Label Schemes**: Develop consistent label naming schemes
4. **Cleanup**: Implement periodic cleanup for unused content
5. **Error Handling**: Always handle store operation errors appropriately
6. **Caching**: Consider implementing local caching for frequently accessed content

## Common Use Cases

### Configuration Storage

```rust
// Store configuration
fn save_config(config: &Config) -> Result<(), String> {
    let config_bytes = serde_json::to_vec(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    let content_ref = store::store(config_bytes)
        .map_err(|e| format!("Failed to store config: {}", e))?;
    
    store::label("app:config".to_string(), content_ref)
        .map_err(|e| format!("Failed to label config: {}", e))?;
    
    Ok(())
}

// Load configuration
fn load_config() -> Result<Config, String> {
    let content_ref = store::get_by_label("app:config".to_string())
        .map_err(|e| format!("Failed to get config reference: {}", e))?;
    
    let config_bytes = store::get(content_ref)
        .map_err(|e| format!("Failed to retrieve config: {}", e))?;
    
    let config: Config = serde_json::from_slice(&config_bytes)
        .map_err(|e| format!("Failed to deserialize config: {}", e))?;
    
    Ok(config)
}
```

### Content Deduplication

```rust
fn store_with_deduplication(data: Vec<u8>) -> Result<ContentRef, String> {
    // Generate a hash to check if the content already exists
    use sha1::{Sha1, Digest};
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let hash = format!("{:x}", hasher.finalize());
    
    // Create a content reference to check existence
    let content_ref = ContentRef { hash };
    
    // Check if the content already exists
    if store::exists(content_ref.clone())? {
        println!("Content already exists in store, reusing existing reference");
        return Ok(content_ref);
    }
    
    // Content doesn't exist, store it
    store::store(data)
}
```

### Versioned Content

```rust
fn store_versioned_content(name: &str, version: &str, data: Vec<u8>) -> Result<(), String> {
    // Store the content
    let content_ref = store::store(data)?;
    
    // Create a versioned label
    let versioned_label = format!("{}:v{}", name, version);
    store::label(versioned_label.clone(), content_ref.clone())?;
    
    // Always update the 'latest' label
    let latest_label = format!("{}:latest", name);
    store::label(latest_label, content_ref)?;
    
    Ok(())
}

fn get_content_version(name: &str, version: &str) -> Result<Vec<u8>, String> {
    let label = format!("{}:v{}", name, version);
    let content_ref = store::get_by_label(label)?;
    store::get(content_ref)
}

fn get_latest_content(name: &str) -> Result<Vec<u8>, String> {
    let label = format!("{}:latest", name);
    let content_ref = store::get_by_label(label)?;
    store::get(content_ref)
}
```

## Related Topics

- [Filesystem Handler](filesystem.md) - Alternative file access mechanism
- [State Management](../core-concepts/state-management.md) - For understanding state chain integration
- [Store System](../core-concepts/store/README.md) - For deeper store concepts
