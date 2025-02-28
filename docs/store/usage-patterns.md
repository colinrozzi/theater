# Store Usage Patterns

This document provides practical examples of common usage patterns for Theater's content store system. These patterns can be applied in both host-side Rust code and in WebAssembly actors.

## 1. Basic Content Storage & Retrieval

The most fundamental pattern is storing and retrieving content:

```rust
// Store some data
let content = "Important information".as_bytes().to_vec();
let content_ref = store.store(content).await?;

// Save the reference for later use
println!("Stored content with ref: {}", content_ref.hash());

// Later, retrieve the content using its reference
let retrieved = store.get(content_ref).await?;
let text = String::from_utf8(retrieved)?;
```

**When to use**: For any data that needs persistence beyond an actor's memory.

## 2. Content Tagging with Labels

Labels provide a way to give meaningful names to content:

```rust
// Store and label in one operation
let config_data = serde_json::to_vec(&config)?;
let content_ref = store.put_at_label("app:config".to_string(), config_data).await?;

// Retrieve by label later
let refs = store.get_by_label("app:config".to_string()).await?;
if let Some(ref_id) = refs.first() {
    let config_data = store.get(ref_id.clone()).await?;
    let config: Config = serde_json::from_slice(&config_data)?;
    // Use the config...
}
```

**When to use**: When you need to retrieve content by name rather than hash reference.

## 3. Actor-Specific Storage

Actors can maintain their own namespace in the store:

```rust
// Store actor-specific data
let actor_id = "7e3f4a21-9c8b-4e5d-a6f7-1b2c3d4e5f6a";
let data = serialize_data(&my_data)?;
let label = format!("actor:{}:state", actor_id);
store.put_at_label(label, data).await?;

// Later, retrieve the actor's data
let label = format!("actor:{}:state", actor_id);
let refs = store.get_by_label(label).await?;
if let Some(ref_id) = refs.first() {
    let data = store.get(ref_id.clone()).await?;
    let my_data = deserialize_data(&data)?;
    // Use the data...
}
```

**When to use**: For actor-specific state that needs to persist across restarts or be accessible by parent actors.

## 4. Versioned Content

Implement simple versioning with labeled content:

```rust
// Store a new version
let document = "Version 2 content".as_bytes().to_vec();
let version = 2;

// Store with version-specific label
let version_label = format!("document:v{}", version);
let content_ref = store.put_at_label(version_label, document.clone()).await?;

// Update the "latest" pointer
store.replace_at_label("document:latest".to_string(), content_ref).await?;

// Retrieve a specific version
let version_label = format!("document:v{}", 1);
let v1_refs = store.get_by_label(version_label).await?;

// Retrieve the latest version
let latest_refs = store.get_by_label("document:latest".to_string()).await?;
```

**When to use**: When you need to maintain history of changes to content.

## 5. Content Deduplication

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
store.label("reference1".to_string(), ref1.clone()).await?;
store.label("reference2".to_string(), ref2.clone()).await?;

// Both labels point to the same content
let content1 = store.get(ref1).await?;
let content2 = store.get(ref2).await?;
assert_eq!(content1, content2);
```

**When to use**: When dealing with potentially duplicate data to save storage space.

## 6. Shared Resources Between Actors

Share data between actors using the store:

```rust
// Actor 1: Create and share data
let shared_data = serialize_data(&some_data)?;
let content_ref = store.put_at_label("shared:resource".to_string(), shared_data).await?;

// Actor 2: Access the shared data
let refs = store.get_by_label("shared:resource".to_string()).await?;
if let Some(ref_id) = refs.first() {
    let shared_data = store.get(ref_id.clone()).await?;
    let some_data = deserialize_data(&shared_data)?;
    // Use the shared data...
}
```

**When to use**: When multiple actors need access to the same data.

## 7. Content-Based Data Structures

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
store.label("list:head".to_string(), node1_ref).await?;

// Traverse the list
let mut current_ref = store.get_by_label("list:head".to_string()).await?[0].clone();
while let Ok(node_bytes) = store.get(current_ref.clone()).await {
    let node: Node = serde_json::from_slice(&node_bytes)?;
    println!("Node value: {}", node.value);
    
    if let Some(next_hash) = node.next {
        current_ref = ContentRef::new(next_hash);
    } else {
        break;
    }
}
```

**When to use**: For implementing persistent data structures where content doesn't change (immutable structures).

## 8. Cached Computation Results

Cache expensive computation results:

```rust
// Function input parameters
let param1 = "value1";
let param2 = 42;

// Create a cache key based on input parameters
let cache_key = format!("cache:func:{}:{}", param1, param2);

// Check if we have a cached result
let cached_refs = store.get_by_label(cache_key.clone()).await?;
if let Some(ref_id) = cached_refs.first() {
    // Return cached result
    let result_bytes = store.get(ref_id.clone()).await?;
    let result: ComputationResult = serde_json::from_slice(&result_bytes)?;
    return Ok(result);
}

// Perform the expensive computation
let result = expensive_computation(param1, param2)?;

// Cache the result
let result_bytes = serde_json::to_vec(&result)?;
store.put_at_label(cache_key, result_bytes).await?;

Ok(result)
```

**When to use**: For memoizing expensive computations that may be repeated with the same inputs.

## 9. Snapshot and Restore

Save and restore application state:

```rust
// Create a snapshot of the current state
let app_state = get_current_state();
let state_bytes = serde_json::to_vec(&app_state)?;

// Store with timestamp
let timestamp = chrono::Utc::now().timestamp();
let snapshot_label = format!("snapshot:{}", timestamp);
let content_ref = store.put_at_label(snapshot_label.clone(), state_bytes).await?;

// Update the "latest" snapshot pointer
store.replace_at_label("snapshot:latest".to_string(), content_ref).await?;

// Later, restore from a snapshot
let refs = store.get_by_label("snapshot:latest".to_string()).await?;
if let Some(ref_id) = refs.first() {
    let state_bytes = store.get(ref_id.clone()).await?;
    let app_state: AppState = serde_json::from_slice(&state_bytes)?;
    restore_state(app_state);
}
```

**When to use**: For applications that need point-in-time recovery or state rollback capabilities.

## 10. Content-Based Messaging

Use the store for message passing with large payloads:

```rust
// Actor 1: Prepare a large message
let large_data = generate_large_data();
let content_ref = store.store(large_data).await?;

// Send only the reference to Actor 2
send_message_to_actor("actor-2", Message {
    command: "process_data",
    data_ref: content_ref.hash(),
});

// Actor 2: Receive message and load data
fn handle_message(msg: Message) -> Result<(), Error> {
    if msg.command == "process_data" {
        // Load the referenced data
        let content_ref = ContentRef::new(msg.data_ref);
        let data = store.get(content_ref).await?;
        
        // Process the data
        process_large_data(data)?;
    }
    
    Ok(())
}
```

**When to use**: When actors need to exchange large amounts of data efficiently.

## 11. Dependency Injection for Actors

Configure actors with store-based dependencies:

```rust
// Store configuration for different environments
let dev_config = Config { url: "http://localhost:8080".to_string(), /* ... */ };
let prod_config = Config { url: "https://production.example.com".to_string(), /* ... */ };

// Store each configuration
let dev_bytes = serde_json::to_vec(&dev_config)?;
let prod_bytes = serde_json::to_vec(&prod_config)?;

store.put_at_label("config:environment:dev".to_string(), dev_bytes).await?;
store.put_at_label("config:environment:prod".to_string(), prod_bytes).await?;

// Actor initialization
fn init(environment: &str) -> Result<(), Error> {
    // Load appropriate configuration
    let config_label = format!("config:environment:{}", environment);
    let refs = store.get_by_label(config_label).await?;
    
    if let Some(ref_id) = refs.first() {
        let config_bytes = store.get(ref_id.clone()).await?;
        let config: Config = serde_json::from_slice(&config_bytes)?;
        
        // Configure actor with loaded config
        configure_services(config);
    }
    
    Ok(())
}
```

**When to use**: For configuring actors differently based on environment or runtime conditions.

## 12. Append-Only Logs

Implement append-only logs with the store:

```rust
// Add a log entry
fn append_log_entry(log_name: &str, entry: LogEntry) -> Result<(), Error> {
    // Serialize the entry
    let entry_bytes = serde_json::to_vec(&entry)?;
    let entry_ref = store.store(entry_bytes).await?;
    
    // Get current log entries
    let log_label = format!("log:{}", log_name);
    let current_refs = store.get_by_label(log_label.clone()).await?;
    
    // Create or update the log file
    if current_refs.is_empty() {
        // New log - initialize with a list containing one entry
        let log = vec![entry_ref.hash()];
        let log_bytes = serde_json::to_vec(&log)?;
        store.put_at_label(log_label, log_bytes).await?;
    } else {
        // Existing log - append to the list
        let mut log: Vec<String> = serde_json::from_slice(&store.get(current_refs[0].clone()).await?)?;
        log.push(entry_ref.hash());
        
        let log_bytes = serde_json::to_vec(&log)?;
        store.replace_content_at_label(log_label, log_bytes).await?;
    }
    
    Ok(())
}

// Read log entries
fn read_log_entries(log_name: &str) -> Result<Vec<LogEntry>, Error> {
    let log_label = format!("log:{}", log_name);
    let refs = store.get_by_label(log_label).await?;
    
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    
    // Get the log index
    let log_bytes = store.get(refs[0].clone()).await?;
    let log_refs: Vec<String> = serde_json::from_slice(&log_bytes)?;
    
    // Load all entries
    let mut entries = Vec::new();
    for entry_hash in log_refs {
        let entry_ref = ContentRef::new(entry_hash);
        let entry_bytes = store.get(entry_ref).await?;
        let entry: LogEntry = serde_json::from_slice(&entry_bytes)?;
        entries.push(entry);
    }
    
    Ok(entries)
}
```

**When to use**: For keeping an ordered record of events or changes that must be preserved in sequence.

## 13. Actor Migration and State Transfer

Migrate actor state during upgrades or transfers:

```rust
// Export actor state for migration
fn export_state_for_migration(actor_id: &str) -> Result<String, Error> {
    // Get current actor state
    let state_label = format!("actor:{}:state", actor_id);
    let refs = store.get_by_label(state_label).await?;
    
    if refs.is_empty() {
        return Err(Error::new("No state found for actor"));
    }
    
    // Create migration package with metadata
    let migration = MigrationPackage {
        actor_id: actor_id.to_string(),
        state_ref: refs[0].hash(),
        timestamp: chrono::Utc::now().timestamp(),
        version: "1.0".to_string(),
    };
    
    let package_bytes = serde_json::to_vec(&migration)?;
    let package_ref = store.store(package_bytes).await?;
    
    // Label the migration package
    let migration_label = format!("migration:{}:{}", actor_id, migration.timestamp);
    store.label(migration_label, package_ref.clone()).await?;
    
    Ok(package_ref.hash())
}

// Import migrated state
fn import_migrated_state(migration_ref: &str) -> Result<(), Error> {
    // Get migration package
    let package_ref = ContentRef::new(migration_ref.to_string());
    let package_bytes = store.get(package_ref).await?;
    let package: MigrationPackage = serde_json::from_slice(&package_bytes)?;
    
    // Get state from the package
    let state_ref = ContentRef::new(package.state_ref);
    let state_bytes = store.get(state_ref).await?;
    
    // Apply to the new actor
    let new_actor_id = generate_new_actor_id();
    let state_label = format!("actor:{}:state", new_actor_id);
    store.put_at_label(state_label, state_bytes).await?;
    
    // Record migration history
    let history_label = format!("actor:{}:migration-history", new_actor_id);
    store.label(history_label, package_ref).await?;
    
    Ok(())
}
```

**When to use**: During actor upgrades, system migrations, or when transferring state between systems.

## 14. Content Synchronization

Synchronize content between different store instances:

```rust
// Export content for synchronization
fn export_content_for_sync(label_pattern: &str) -> Result<Vec<u8>, Error> {
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
        let refs = store.get_by_label(label.clone()).await?;
        if !refs.is_empty() {
            // Add the label and content to the package
            let content_ref = refs[0].clone();
            let content = store.get(content_ref.clone()).await?;
            
            sync_package.labels.push((label.clone(), content_ref.hash()));
            sync_package.content.insert(content_ref.hash(), content);
        }
    }
    
    // Serialize the package
    let package_bytes = bincode::serialize(&sync_package)?;
    
    Ok(package_bytes)
}

// Import synchronized content
fn import_sync_package(package_bytes: Vec<u8>) -> Result<(), Error> {
    // Deserialize the package
    let package: SyncPackage = bincode::deserialize(&package_bytes)?;
    
    // Store all content
    for (hash, content) in package.content {
        let content_ref = ContentRef::new(hash.clone());
        if !store.exists(content_ref.clone()).await? {
            store.store(content).await?;
        }
    }
    
    // Apply all labels
    for (label, hash) in package.labels {
        let content_ref = ContentRef::new(hash);
        store.label(label, content_ref).await?;
    }
    
    Ok(())
}
```

**When to use**: When synchronizing data between different Theater instances or for backup/restore.

## 15. Resource Pools

Implement reusable resource pools with the store:

```rust
// Claim a resource from a pool
fn claim_resource(pool_name: &str) -> Result<Resource, Error> {
    let pool_label = format!("resource-pool:{}", pool_name);
    
    // Get the current pool state
    let pool_refs = store.get_by_label(pool_label.clone()).await?;
    if pool_refs.is_empty() {
        return Err(Error::new("Resource pool not found"));
    }
    
    // Update pool atomically
    let pool_bytes = store.get(pool_refs[0].clone()).await?;
    let mut pool: ResourcePool = serde_json::from_slice(&pool_bytes)?;
    
    // Find an available resource
    if let Some(resource) = pool.claim_available_resource() {
        // Update the pool
        let updated_pool_bytes = serde_json::to_vec(&pool)?;
        store.replace_content_at_label(pool_label, updated_pool_bytes).await?;
        
        Ok(resource)
    } else {
        Err(Error::new("No available resources in the pool"))
    }
}

// Release a resource back to the pool
fn release_resource(pool_name: &str, resource_id: &str) -> Result<(), Error> {
    let pool_label = format!("resource-pool:{}", pool_name);
    
    // Get the current pool state
    let pool_refs = store.get_by_label(pool_label.clone()).await?;
    if pool_refs.is_empty() {
        return Err(Error::new("Resource pool not found"));
    }
    
    // Update pool atomically
    let pool_bytes = store.get(pool_refs[0].clone()).await?;
    let mut pool: ResourcePool = serde_json::from_slice(&pool_bytes)?;
    
    // Release the resource
    pool.release_resource(resource_id);
    
    // Update the pool
    let updated_pool_bytes = serde_json::to_vec(&pool)?;
    store.replace_content_at_label(pool_label, updated_pool_bytes).await?;
    
    Ok(())
}
```

**When to use**: For managing shared, limited resources among multiple actors.

## Summary

These patterns illustrate the versatility of Theater's content store and can be combined or adapted to fit various application requirements. The content-addressable nature of the store, combined with the labeling system, provides a foundation for building sophisticated persistence mechanisms while maintaining the integrity guarantees that Theater provides.
