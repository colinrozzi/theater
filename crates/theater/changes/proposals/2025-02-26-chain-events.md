
## Description

The chain is the core purpose of the system. In the future, it will be the core data structure that we will use to trace errors as they occur and move through the system, that we will use to verify foreign code, and that we will use as the basis for decentralized trust.

Currently, each entry in the chain is a simple string, and events are created with just an event type and data. This approach has been sufficient for basic functionality but limits our ability to effectively debug and monitor the system. This proposal outlines four key improvements:

1. **Structured Event Objects**: Move from submitting text for chain events to having each host handler create structured event objects for different event types, improving type safety and making events easier to work with.

2. **Enhanced Event Representation**: Improve how each event is represented textually to make it more readable and informative for debugging purposes.

3. **Real-time Event Notification**: Build a mechanism to notify the theater_runtime when events occur, enabling immediate reaction to state changes.

4. **CLI Event Subscription**: Create a dedicated command in the CLI tool to subscribe to actor events and display them in real-time.

### Why These Changes Are Necessary

- **Type Safety**: Structured event objects will provide better type safety and eliminate runtime errors from improper event data.
- **Debugging**: Better event structure and textual representation will significantly improve debugging workflows.
- **Monitoring**: Real-time event subscription will allow developers to observe actor behavior as it happens.
- **Extensibility**: These changes lay groundwork for future features like event filtering, aggregation, and cross-actor event tracing.
- **Decentralized Trust**: Enhanced chain events are a prerequisite for implementing verifiable and auditable actor interactions.

### Expected Benefits

- Type-safe event creation and handling, reducing runtime errors
- Better compile-time checks for event data correctness
- Improved developer experience when debugging actor behavior
- Faster identification of issues through real-time event monitoring
- Better visibility into the internal state changes of actors
- Foundation for advanced features like event filtering and complex event processing
- More structured data for analytics and monitoring tools

### Potential Risks

- Performance impact if event serialization is computationally expensive
- Migration effort required to update all handlers to use the new event system
- Need to update all code that interacts with chain events
- Potential for increased memory usage with more detailed event data
- Initial complexity increase as the system transitions to typed events

### Alternatives Considered

- Continuing with string-based events and improving only the formatting (rejected due to missed opportunity for type safety)
- Using a dynamic typing approach for events (rejected due to lack of compile-time checking)
- Simple logging without chain integration (rejected due to loss of verification capabilities)
- External event monitoring system (rejected due to increased complexity and deployment overhead)
- Storing events in a separate data structure (rejected to maintain the integrity of the hash chain)

## Technical Approach

### 1. Structured Event Objects

We'll create a new `events` module with a trait for events and implementations for each handler type. First, let's define the base event trait:

```rust
// src/events/mod.rs
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Base trait for all chain events
pub trait ChainEventData: Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static {
    /// The event type identifier
    fn event_type(&self) -> &'static str;
    
    /// Human-readable description of the event
    fn description(&self) -> String;
    
    /// Convert to JSON
    fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

// Import specific event modules
pub mod http;
pub mod message;
pub mod filesystem;
pub mod runtime;
pub mod supervisor;
```

Now, let's implement some specific event types for different handlers:

```rust
// src/events/http.rs
use super::ChainEventData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpEvent {
    RequestReceived {
        method: String,
        path: String,
        headers_count: usize,
        body_size: usize,
    },
    ResponseSent {
        status: u16,
        headers_count: usize,
        body_size: usize,
    },
    Error {
        message: String,
        code: Option<u16>,
    },
}

impl ChainEventData for HttpEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::RequestReceived { .. } => "http.request_received",
            Self::ResponseSent { .. } => "http.response_sent",
            Self::Error { .. } => "http.error",
        }
    }
    
    fn description(&self) -> String {
        match self {
            Self::RequestReceived { method, path, headers_count, body_size } => {
                format!("HTTP {} request to {} ({} headers, {} bytes)", 
                    method, path, headers_count, body_size)
            },
            Self::ResponseSent { status, headers_count, body_size } => {
                format!("HTTP {} response ({} headers, {} bytes)", 
                    status, headers_count, body_size)
            },
            Self::Error { message, code } => {
                if let Some(code) = code {
                    format!("HTTP error {}: {}", code, message)
                } else {
                    format!("HTTP error: {}", message)
                }
            },
        }
    }
}
```

We'll need to update the `ChainEvent` struct to work with these typed events:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct ChainEvent {
    pub hash: Vec<u8>,
    #[component(name = "parent-hash")]
    pub parent_hash: Option<Vec<u8>>,
    #[component(name = "event-type")]
    pub event_type: String,
    pub data: Vec<u8>,
    pub timestamp: u64,
    // Optional human-readable description
    pub description: Option<String>,
}

impl ChainEvent {
    /// Create a new event from a typed event data object
    pub fn new<T: ChainEventData>(
        event_data: T, 
        parent_hash: Option<Vec<u8>>
    ) -> Result<Self, serde_json::Error> {
        let data = event_data.to_json()?;
        let event_type = event_data.event_type().to_string();
        let description = Some(event_data.description());
        
        // Hash calculation will be done in add_event
        // This is just a placeholder
        let hash = Vec::new();
        
        Ok(Self {
            hash,
            parent_hash,
            event_type,
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description,
        })
    }
    
    /// Try to parse the event data as a specific event type
    pub fn parse_data<T: ChainEventData>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.data)
    }
}
```

We'll modify the `StateChain` to work with these typed events:

```rust
impl StateChain {
    // ...existing code...
    
    /// Add a typed event to the chain
    pub fn add_typed_event<T: ChainEventData>(&mut self, event_data: T) -> Result<ChainEvent, serde_json::Error> {
        let event = ChainEvent::new(event_data, self.current_hash.clone())?;
        self.finalize_and_store_event(event)
    }
    
    /// Internal method to finalize event (add hash) and store it
    fn finalize_and_store_event(&mut self, mut event: ChainEvent) -> Result<ChainEvent, serde_json::Error> {
        let mut hasher = Sha1::new();
        
        // Hash previous state + event data
        if let Some(prev_hash) = &self.current_hash {
            hasher.update(prev_hash);
        }
        hasher.update(&event.data);
        
        // Set the hash
        event.hash = hasher.finalize().to_vec();
        
        // Store the event
        self.events.push(event.clone());
        self.current_hash = Some(event.hash.clone());
        
        // Notify runtime if callback is set
        if let Some(callback) = &self.event_callback {
            if let Err(err) = callback.send(event.clone()) {
                tracing::warn!("Failed to notify runtime of event: {}", err);
            }
        }
        
        Ok(event)
    }
    
    // Legacy method for backward compatibility
    pub fn add_event(&mut self, event_type: String, data: Vec<u8>) -> ChainEvent {
        let mut event = ChainEvent {
            hash: Vec::new(),  // Will be set below
            parent_hash: self.current_hash.clone(),
            event_type,
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            description: None,
        };
        
        self.finalize_and_store_event(event).unwrap()
    }
}
```

### 2. Enhanced Event Representation

We'll update the `Display` implementation for `ChainEvent` to use the new structure:

```rust
impl fmt::Display for ChainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format timestamp as human-readable date with millisecond precision
        let datetime = chrono::DateTime::from_timestamp(self.timestamp as i64, 0)
            .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
        let formatted_time = datetime.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        
        // Format hash as short hex string (first 7 characters)
        let hash_str = self.hash.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let short_hash = if hash_str.len() > 7 {
            &hash_str[0..7]
        } else {
            &hash_str
        };
        
        // Format parent hash if it exists
        let parent_str = match &self.parent_hash {
            Some(ph) => {
                let ph_str = ph.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                if ph_str.len() > 7 {
                    format!("(parent: {}...)", &ph_str[0..7])
                } else {
                    format!("(parent: {})", ph_str)
                }
            },
            None => "(root)".to_string(),
        };
        
        // Use the description if available
        let content = if let Some(desc) = &self.description {
            desc.clone()
        } else {
            // Format data preview, attempting JSON formatting if possible
            if let Ok(text) = std::str::from_utf8(&self.data) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                    if json.is_object() && json.as_object().unwrap().len() <= 3 {
                        // For small JSON objects, inline them
                        serde_json::to_string(&json).unwrap_or_else(|_| text.to_string())
                    } else {
                        // For larger JSON, just show a preview
                        let preview = if text.len() > 30 {
                            format!("{}...", &text[0..27])
                        } else {
                            text.to_string()
                        };
                        format!("'{}'", preview)
                    }
                } else {
                    // Not JSON, just show text preview
                    let preview = if text.len() > 30 {
                        format!("{}...", &text[0..27])
                    } else {
                        text.to_string()
                    };
                    format!("'{}'", preview)
                }
            } else {
                // Binary data
                format!("{} bytes of binary data", self.data.len())
            }
        };
        
        write!(f, "[{}] Event[{}] {} {} {}", 
            formatted_time,
            short_hash,
            parent_str,
            style(&self.event_type).cyan(),
            content
        )
    }
}
```

### 3. Real-time Event Notification

We'll add a notification system in `StateChain`:

```rust
pub fn set_event_callback(&mut self, 
    callback: mpsc::Sender<ChainEvent>
) {
    self.event_callback = Some(callback);
}
```

And add a field to store it:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChain {
    events: Vec<ChainEvent>,
    current_hash: Option<Vec<u8>>,
    #[serde(skip)]
    event_callback: Option<mpsc::Sender<ChainEvent>>,
}
```

### 4. CLI Event Subscription

We'll enhance the existing `ActorCommands::Subscribe` command in the CLI to include filtering options:

```rust
/// Subscribe to actor events
Subscribe {
    /// Actor ID to subscribe to
    #[arg(value_name = "ACTOR_ID")]
    id: Option<String>,
    
    /// Filter events by type (supports glob patterns like http.*)
    #[arg(short, long)]
    event_type: Option<String>,
    
    /// Show detailed event information including raw data
    #[arg(short, long)]
    detailed: bool,
    
    /// Maximum number of events to show (0 for unlimited)
    #[arg(short, long, default_value = "0")]
    limit: usize,
},
```



### 5. Actor Handle and Store Integration

We need to update the `ActorHandle` and `ActorStore` to work with the new event system:

```rust
// Update ActorStore
impl ActorStore {
    // Replace the old record_event method with these typed methods
    pub fn record_http_event(&self, event: HttpEvent) -> ChainEvent {
        let mut chain = self.chain.lock().unwrap();
        chain.add_http_event(event)
    }
    
    pub fn record_filesystem_event(&self, event: FilesystemEvent) -> ChainEvent {
        let mut chain = self.chain.lock().unwrap();
        chain.add_filesystem_event(event)
    }
    
    // Add similar methods for other event types
    
    // Generic method for any event type
    pub fn record_event(&self, event: ChainEvent) -> ChainEvent {
        let mut chain = self.chain.lock().unwrap();
        chain.add_event(event)
    }
    
    // Other methods remain largely the same...
}

// Update ActorHandle
impl ActorHandle {
    // Add convenience methods for recording events
    pub fn record_http_event(&self, event: HttpEvent) -> Result<()> {
        // Execute an operation to record the event
        self.execute_operation(ActorOperation::RecordHttpEvent { event })
            .await
            .map(|_| ())
    }
    
    pub fn record_filesystem_event(&self, event: FilesystemEvent) -> Result<()> {
        self.execute_operation(ActorOperation::RecordFilesystemEvent { event })
            .await
            .map(|_| ())
    }
    
    // Add similar methods for other event types
}

// Update ActorOperation enum
pub enum ActorOperation {
    // Add new operations for different event types
    RecordHttpEvent { event: HttpEvent },
    RecordFilesystemEvent { event: FilesystemEvent },
    // etc.
    
    // Existing operations...
    GetState { response_tx: oneshot::Sender<Result<Option<Vec<u8>>, ActorError>> },
    GetChain { response_tx: oneshot::Sender<Result<Vec<ChainEvent>, ActorError>> },
    GetMetrics { response_tx: oneshot::Sender<Result<ActorMetrics, ActorError>> },
}
```

### 6. CLI Support for the New Event System

Finally, we'll update the CLI event subscription to work with the new typed events:

```rust
async fn subscribe_to_actor(id_opt: Option<String>, event_type_pattern: Option<String>, detailed: bool, limit: usize, address: &str) -> Result<()> {
    println!("{}", style("Theater Event Monitor").bold().cyan());

    let actor_id = select_actor(id_opt, address).await?;
    let mut framed = connect_to_server(address).await?;

    println!(
        "{} Subscribing to events for actor {}",
        style("INFO:").blue().bold(),
        style(actor_id.clone()).green()
    );

    // Subscribe to# Chain Event Improvements
```    

## Implementation Steps

1. **Create Structured Event System**:
   - Create a new `events` module with the `ChainEventData` trait
   - Define the new `EventData` enum for all event types
   - Implement specific event types for each handler
   - Completely replace the existing `ChainEvent` structure

2. **Redesign Chain Module**:
   - Redesign `StateChain` to work with typed events
   - Remove legacy string-based event methods
   - Implement verification that works with the new event system
   - Add notification callback mechanism

3. **Update Host Handlers**:
   - Modify each handler to create appropriate event objects
   - Switch all event recording to use the typed system
   - Update event-related functions throughout the system

4. **Enhance Event Representation**:
   - Update the `fmt::Display` implementation for `ChainEvent`
   - Implement proper formatting for each event type
   - Add color-coding for different event categories

5. **Update Actor Runtime and Store**:
   - Modify `ActorStore` to work with typed events
   - Update `ActorHandle` to provide convenient event recording methods
   - Modify `ActorRuntime` to handle the new event types

6. **Update CLI and Management Interface**:
   - Enhance the CLI subscription command with filtering capabilities
   - Implement glob-style pattern matching for event types
   - Add detailed display options for different event types
   - Update all management commands that interact with events

## Working Notes

### Event Type Design Considerations

We should implement specific event types for each handler type in the system:

1. **HTTP Events**:
   - RequestReceived: method, path, headers, body_size
   - ResponseSent: status, headers, body_size
   - Error: message, code

2. **Filesystem Events**:
   - FileRead: path, bytes_read, success
   - FileWrite: path, bytes_written, success
   - DirectoryCreated: path, success
   - DirectoryListed: path, entries_count, success
   - Error: operation, path, message

3. **Message Events**:
   - MessageReceived: sender, message_type, size
   - MessageSent: recipient, message_type, size
   - Error: message, context

4. **Runtime Events**:
   - Startup: config_summary
   - Shutdown: reason
   - StateChange: old_state, new_state
   - Error: message, context
   - Log: level, message

5. **Supervisor Events**:
   - ChildSpawned: child_id, manifest_path
   - ChildStopped: child_id, reason
   - ChildRestarted: child_id, reason
   - Error: message, child_id

This approach gives us well-defined event types for each aspect of the system.

### Performance Considerations

- Serialization of structured events might be more expensive than simple strings
- Consider lazy serialization to minimize overhead when events aren't monitored
- For high-frequency events, consider buffering or sampling to reduce overhead
- Event subscription should support filtering at the source rather than client-side only

### CLI Design

The CLI improvements should include:

- Color-coding events by type (HTTP=cyan, filesystem=green, errors=red, etc.)
- Support for glob-style pattern matching ("http.*", "*.error", etc.)
- Different verbosity levels (normal vs. detailed)
- Option to output events as JSON for machine consumption
- Ability to follow events in real-time or show a limited history

### Migration Strategy

Since we're making breaking changes, we should:

1. First implement the new event system alongside the old one
2. Update one handler at a time to use the new system
3. Once all handlers are updated, remove the old system
4. Update documentation and examples to reflect the new approach

## Final Notes

[To be filled in after implementation]
