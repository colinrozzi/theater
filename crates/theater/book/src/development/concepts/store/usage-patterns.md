# Store Usage Patterns

This document provides practical examples of common usage patterns for Theater's content store system. These patterns can be applied in both host-side Rust code and in WebAssembly actors.

## 1. Creating and Managing Stores

The store system supports multiple store instances, each with a unique ID:

```rust
// Create a new store with a generated UUID
let store = ContentStore::new();
println!("Created store with ID: {}", store.id());

// Create a store with a specific ID
let store = ContentStore::from_id("my-store-id");

// For actors using host functions
let store_id = store::new()?;
```

**When to use**: When you need separate storage instances for different use cases or actors.

## 2. Basic Content Storage & Retrieval

The most fundamental pattern is storing and retrieving content:

```rust
// Store some data
let content = "Important information".as_bytes().to_vec();
let content_ref = store.store(content).await?;

// Save the reference for later use
println!("Stored content with hash: {}", content_ref.hash());

// Later, retrieve the content using its reference
let retrieved = store.get(&content_ref).await?;
let text = String::from_utf8(retrieved)?;
```

**When to use**: For any data that needs persistence beyond an actor's memory.

## 3. Content Tagging with Labels

Labels provide a way to give meaningful names to content:

```rust
// Store and label in one operation
let config_data = serde_json::to_vec(&config)?;
let content_ref = store.store_at_label("app:config", config_data).await?;

// Retrieve by label later
let content_ref_opt = store.get_by_label("app:config").await?;
if let Some(content_ref) = content_ref_opt {
    let config_data = store.get(&content_ref).await?;
    let config: Config = serde_json::from_slice(&config_data)?;
    // Use the config...
}

// Alternative: directly get content by label
let config_data_opt = store.get_content_by_label("app:config").await?;
if let Some(config_data) = config_data_opt {
    let config: Config = serde_json::from_slice(&config_data)?;
    // Use the config...
}
```

**When to use**: When you need to retrieve content by name rather than hash reference.

## 4. Actor-Specific Storage

Actors can maintain their own namespace in the store:

```rust
// Store actor-specific data
let actor_id = "7e3f4a21-9c8b-4e5d-a6f7-1b2c3d4e5f6a";
let data = serialize_data(&my_data)?;
let label = format!("actor:{}:state", actor_id);
store.store_at_label(&label, data).await?;

// Later, retrieve the actor's data
let label = format!("actor:{}:state", actor_id);
let content_ref_opt = store.get_by_label(&label).await?;
if let Some(content_ref) = content_ref_opt {
    let data = store.get(&content_ref).await?;
    let my_data = deserialize_data(&data)?;
    // Use the data...
}
```

**When to use**: For actor-specific state that needs to persist across restarts or be accessible by parent actors.

## 5. Versioned Content

Implement simple versioning with labeled content:

```rust
// Store a new version
let document = "Version 2 content".as_bytes().to_vec();
let version = 2;

// Store with version-specific label
let version_label = format!("document:v{}", version);
let content_ref = store.store_at_label(&version_label, document.clone()).await?;

// Update the "latest" pointer
store.replace_at_label("document:latest", &content_ref).await?;

// Retrieve a specific version
let version_label = format!("document:v{}", 1);
let v1_ref_opt = store.get_by_label(&version_label).await?;

// Retrieve the latest version
let latest_ref_opt = store.get_by_label("document:latest").await?;
```

**When to use**: When you need to maintain history of changes to content.

## 6. Content Deduplication

Take advantage of content-addressing for automatic deduplication:

```rust
// Store two identical pieces of content
let content1 = "Duplicate content".as_bytes().to_vec();
let content2 = "Duplicate content".as_bytes().to_vec();

let ref1 = store.store(content1).await?;
let ref2 = store.store(content2).await?;

// Even though we stored twice, they have the same hash
assert_eq!(ref1.hash(), ref2.hash());

// Label both references
store.label("reference1", &ref1).await?;
store.label("reference2", &ref2).await?;

// Both labels point to the same content
let content1 = store.get(&ref1).await?;
let content2 = store.get(&ref2).await?;
assert_eq!(content1, content2);
```

**When to use**: When dealing with potentially duplicate data to save storage space.

## 7. Shared Resources Between Actors

Share data between actors using the store:

```rust
// Actor 1: Create and share data
let store = ContentStore::new();
let store_id = store.id().to_string();
let shared_data = serialize_data(&some_data)?;
let content_ref = store.store_at_label("shared:resource", shared_data).await?;

// Send the store_id to Actor 2
send_to_actor_2(store_id);

// Actor 2: Access the shared data
let store = ContentStore::from_id(&received_store_id);
let content_ref_opt = store.get_by_label("shared:resource").await?;
if let Some(content_ref) = content_ref_opt {
    let shared_data = store.get(&content_ref).await?;
    let some_data = deserialize_data(&shared_data)?;
    // Use the shared data...
}
```

**When to use**: When multiple actors need access to the same data.

## 8. Content-Based Data Structures

Implement data structures using content references:

```rust
// A simple linked list node
#[derive(Serialize, Deserialize)]
struct Node {
    value: String,
    next: Option<String>, // ContentRef hash as string
}

// Create and store a linked list
let node3 = Node {
    value: "Node 3".to_string(),
    next: None,
};

// Serialize and store node 3
let node3_bytes = serde_json::to_vec(&node3)?;
let node3_ref = store.store(node3_bytes).await?;

// Create and store node 2, linking to node 3
let node2 = Node {
    value: "Node 2".to_string(),
    next: Some(node3_ref.hash().to_string()),
};
let node2_bytes = serde_json::to_vec(&node2)?;
let node2_ref = store.store(node2_bytes).await?;

// Create and store node 1, linking to node 2
let node1 = Node {
    value: "Node 1".to_string(),
    next: Some(node2_ref.hash().to_string()),
};
let node1_bytes = serde_json::to_vec(&node1)?;
let node1_ref = store.store(node1_bytes).await?;

// Label the head of the list
store.label("list:head", &node1_ref).await?;

// Traverse the list
let mut current_ref_opt = store.get_by_label("list:head").await?;
while let Some(current_ref) = current_ref_opt {
    let node_bytes = store.get(&current_ref).await?;
    let node: Node = serde_json::from_slice(&node_bytes)?;
    println!("Node value: {}", node.value);
    
    if let Some(next_hash) = node.next {
        current_ref_opt = Some(ContentRef::new(next_hash));
    } else {
        current_ref_opt = None;
    }
}
```

**When to use**: For implementing persistent data structures where content doesn't change (immutable structures).

## 9. Cached Computation Results

Cache expensive computation results:

```rust
// Function input parameters
let param1 = "value1";
let param2 = 42;

// Create a cache key based on input parameters
let cache_key = format!("cache:func:{}:{}", param1, param2);

// Check if we have a cached result
let cached_content_opt = store.get_content_by_label(&cache_key).await?;
if let Some(result_bytes) = cached_content_opt {
    // Return cached result
    let result: ComputationResult = serde_json::from_slice(&result_bytes)?;
    return Ok(result);
}

// Perform the expensive computation
let result = expensive_computation(param1, param2)?;

// Cache the result
let result_bytes = serde_json::to_vec(&result)?;
store.store_at_label(&cache_key, result_bytes).await?;

Ok(result)
```

**When to use**: For memoizing expensive computations that may be repeated with the same inputs.

## 10. Snapshot and Restore

Save and restore application state:

```rust
// Create a snapshot of the current state
let app_state = get_current_state();
let state_bytes = serde_json::to_vec(&app_state)?;

// Store with timestamp
let timestamp = chrono::Utc::now().timestamp();
let snapshot_label = format!("snapshot:{}", timestamp);
let content_ref = store.store_at_label(&snapshot_label, state_bytes).await?;

// Update the "latest" snapshot pointer
store.replace_at_label("snapshot:latest", &content_ref).await?;

// Later, restore from a snapshot
let ref_opt = store.get_by_label("snapshot:latest").await?;
if let Some(content_ref) = ref_opt {
    let state_bytes = store.get(&content_ref).await?;
    let app_state: AppState = serde_json::from_slice(&state_bytes)?;
    restore_state(app_state);
}
```

**When to use**: For applications that need point-in-time recovery or state rollback capabilities.

## 11. Content-Based Messaging

Use the store for message passing with large payloads:

```rust
// Actor 1: Prepare a large message
let store = ContentStore::new();
let store_id = store.id().to_string();
let large_data = generate_large_data();
let content_ref = store.store(large_data).await?;

// Send only the reference and store ID to Actor 2
send_message_to_actor("actor-2", Message {
    command: "process_data",
    store_id: store_id,
    data_ref: content_ref.hash(),
});

// Actor 2: Receive message and load data
fn handle_message(msg: Message) -> Result<(), Error> {
    if msg.command == "process_data" {
        // Create store instance from the ID
        let store = ContentStore::from_id(&msg.store_id);
        
        // Load the referenced data
        let content_ref = ContentRef::new(msg.data_ref);
        let data = store.get(&content_ref).await?;
        
        // Process the data
        process_large_data(data)?;
    }
    
    Ok(())
}
```

**When to use**: When actors need to exchange large amounts of data efficiently.

## 12. Dependency Injection for Actors

Configure actors with store-based dependencies:

```rust
// Create store for configuration
let store = ContentStore::new();
let store_id = store.id().to_string();

// Store configuration for different environments
let dev_config = Config { url: "http://localhost:8080".to_string(), /* ... */ };
let prod_config = Config { url: "https://production.example.com".to_string(), /* ... */ };

// Store each configuration
let dev_bytes = serde_json::to_vec(&dev_config)?;
let prod_bytes = serde_json::to_vec(&prod_config)?;

store.store_at_label("config:environment:dev", dev_bytes).await?;
store.store_at_label("config:environment:prod", prod_bytes).await?;

// Send store ID to actors
spawn_actor(actor_params.with_store_id(store_id));

// Actor initialization
fn init(store_id: &str, environment: &str) -> Result<(), Error> {
    // Create store from ID
    let store = ContentStore::from_id(store_id);
    
    // Load appropriate configuration
    let config_label = format!("config:environment:{}", environment);
    let config_opt = store.get_content_by_label(&config_label).await?;
    
    if let Some(config_bytes) = config_opt {
        let config: Config = serde_json::from_slice(&config_bytes)?;
        
        // Configure actor with loaded config
        configure_services(config);
    }
    
    Ok(())
}
```

**When to use**: For configuring actors differently based on environment or runtime conditions.

## 13. Append-Only Logs

Implement append-only logs with the store:

```rust
// Add a log entry
async fn append_log_entry(store: &ContentStore, log_name: &str, entry: LogEntry) -> Result<(), Error> {
    // Serialize the entry
    let entry_bytes = serde_json::to_vec(&entry)?;
    let entry_ref = store.store(entry_bytes).await?;
    
    // Get current log entries
    let log_label = format!("log:{}", log_name);
    let current_content_opt = store.get_content_by_label(&log_label).await?;
    
    // Create or update the log file
    if let Some(log_bytes) = current_content_opt {
        // Existing log - append to the list
        let mut log: Vec<String> = serde_json::from_slice(&log_bytes)?;
        log.push(entry_ref.hash().to_string());
        
        let updated_log_bytes = serde_json::to_vec(&log)?;
        store.replace_content_at_label(&log_label, updated_log_bytes).await?;
    } else {
        // New log - initialize with a list containing one entry
        let log = vec![entry_ref.hash().to_string()];
        let log_bytes = serde_json::to_vec(&log)?;
        store.store_at_label(&log_label, log_bytes).await?;
    }
    
    Ok(())
}

// Read log entries
async fn read_log_entries(store: &ContentStore, log_name: &str) -> Result<Vec<LogEntry>, Error> {
    let log_label = format!("log:{}", log_name);
    let log_content_opt = store.get_content_by_label(&log_label).await?;
    
    if let Some(log_bytes) = log_content_opt {
        // Get the log index
        let log_refs: Vec<String> = serde_json::from_slice(&log_bytes)?;
        
        // Load all entries
        let mut entries = Vec::new();
        for entry_hash in log_refs {
            let entry_ref = ContentRef::new(entry_hash);
            let entry_bytes = store.get(&entry_ref).await?;
            let entry: LogEntry = serde_json::from_slice(&entry_bytes)?;
            entries.push(entry);
        }
        
        Ok(entries)
    } else {
        Ok(Vec::new())
    }
}
```

**When to use**: For keeping an ordered record of events or changes that must be preserved in sequence.

## 14. Actor Migration and State Transfer

Migrate actor state during upgrades or transfers:

```rust
// Export actor state for migration
async fn export_state_for_migration(store: &ContentStore, actor_id: &str) -> Result<String, Error> {
    // Get current actor state
    let state_label = format!("actor:{}:state", actor_id);
    let state_ref_opt = store.get_by_label(&state_label).await?;
    
    if let Some(state_ref) = state_ref_opt {
        // Create migration package with metadata
        let migration = MigrationPackage {
            actor_id: actor_id.to_string(),
            state_ref: state_ref.hash().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            version: "1.0".to_string(),
        };
        
        let package_bytes = serde_json::to_vec(&migration)?;
        let package_ref = store.store(package_bytes).await?;
        
        // Label the migration package
        let migration_label = format!("migration:{}:{}", actor_id, migration.timestamp);
        store.label(&migration_label, &package_ref).await?;
        
        Ok(package_ref.hash().to_string())
    } else {
        Err(Error::new("No state found for actor"))
    }
}

// Import migrated state
async fn import_migrated_state(store: &ContentStore, migration_hash: &str) -> Result<(), Error> {
    // Get migration package
    let package_ref = ContentRef::new(migration_hash.to_string());
    let package_bytes = store.get(&package_ref).await?;
    let package: MigrationPackage = serde_json::from_slice(&package_bytes)?;
    
    // Get state from the package
    let state_ref = ContentRef::new(package.state_ref);
    let state_bytes = store.get(&state_ref).await?;
    
    // Apply to the new actor
    let new_actor_id = generate_new_actor_id();
    let state_label = format!("actor:{}:state", new_actor_id);
    store.store_at_label(&state_label, state_bytes).await?;
    
    // Record migration history
    let history_label = format!("actor:{}:migration-history", new_actor_id);
    store.label(&history_label, &package_ref).await?;
    
    Ok(())
}
```

**When to use**: During actor upgrades, system migrations, or when transferring state between systems.

## 15. Content Synchronization

Synchronize content between different store instances:

```rust
// Export content for synchronization
async fn export_content_for_sync(store: &ContentStore, label_pattern: &str) -> Result<Vec<u8>, Error> {
    // Find all matching labels
    let all_labels = store.list_labels().await?;
    let matching_labels: Vec<String> = all_labels
        .into_iter()
        .filter(|l| l.starts_with(label_pattern))
        .collect();
    
    // Collect all content references
    let mut sync_package = SyncPackage {
        labels: Vec::new(),
        content: HashMap::new(),
    };
    
    for label in &matching_labels {
        let ref_opt = store.get_by_label(label).await?;
        if let Some(content_ref) = ref_opt {
            // Add the label and content to the package
            let content = store.get(&content_ref).await?;
            
            sync_package.labels.push((label.clone(), content_ref.hash().to_string()));
            sync_package.content.insert(content_ref.hash().to_string(), content);
        }
    }
    
    // Serialize the package
    let package_bytes = bincode::serialize(&sync_package)?;
    
    Ok(package_bytes)
}

// Import synchronized content
async fn import_sync_package(store: &ContentStore, package_bytes: Vec<u8>) -> Result<(), Error> {
    // Deserialize the package
    let package: SyncPackage = bincode::deserialize(&package_bytes)?;
    
    // Store all content
    for (hash, content) in package.content {
        let content_ref = ContentRef::new(hash.clone());
        if !store.exists(&content_ref).await {
            store.store(content).await?;
        }
    }
    
    // Apply all labels
    for (label, hash) in package.labels {
        let content_ref = ContentRef::new(hash);
        store.label(&label, &content_ref).await?;
    }
    
    Ok(())
}
```

**When to use**: When synchronizing data between different Theater instances or for backup/restore.

## Summary

These patterns illustrate the versatility of Theater's content store and can be combined or adapted to fit various application requirements. The content-addressable nature of the store, combined with the labeling system, provides a foundation for building sophisticated persistence mechanisms while maintaining the integrity guarantees that Theater provides.