use crate::chain::ChainEvent;
use serde::{Deserialize, Serialize};

/// # Chain Event Data
///
/// `ChainEventData` is the base structure for all typed events in the Theater system.
/// It wraps specific event data with common metadata like event type, timestamp, and
/// optional human-readable description.
///
/// ## Purpose
///
/// This struct serves as the bridge between strongly-typed event data and the generic
/// chain event system. It provides a standardized way to attach metadata to any type
/// of event, ensuring that all events in the system have consistent properties
/// regardless of their specific payload type.
///
/// ## Example
///
/// ```rust
/// use theater::events::{ChainEventData, EventData};
/// use theater::events::runtime::RuntimeEventData;
/// use chrono::Utc;
///
/// // Create a runtime initialization event
/// let event_data = ChainEventData {
///     event_type: "actor.init".to_string(),
///     data: EventData::Runtime(RuntimeEventData::Init {
///         params: vec![1, 2, 3],
///     }),
///     timestamp: Utc::now().timestamp() as u64,
///     description: Some("Actor initialized".to_string()),
/// };
/// ```
///
/// ## Implementation Notes
///
/// The `ChainEventData` structure is designed to be serializable, allowing events
/// to be stored, transmitted over the network, or saved to a file. It can be
/// converted to a `ChainEvent` for inclusion in an actor's event chain using
/// the `to_chain_event` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEventData {
    /// The type identifier for this event, used for filtering and routing.
    /// This should be a dot-separated string like "subsystem.action".
    pub event_type: String,
    /// The specific event data payload, containing domain-specific information.
    pub data: EventData,
    /// Unix timestamp (in seconds) when the event was created.
    pub timestamp: u64,
    /// Optional human-readable description of the event for logging and debugging.
    pub description: Option<String>,
}

/// # Event Data
///
/// `EventData` is an enum that represents the various types of events that can occur
/// in the Theater system, organized by subsystem. Each variant contains a specific
/// event data structure from the corresponding subsystem.
///
/// ## Purpose
///
/// This enum provides a type-safe way to handle events from different subsystems
/// within a single event chain. It allows for pattern matching on event types and
/// encapsulates the domain-specific data for each type of event.
///
/// ## Example
///
/// ```rust
/// use theater::events::{EventData, ChainEventData};
/// use theater::events::http::HttpEventData;
///
/// // Process events based on their type
/// fn handle_event(event: &ChainEventData) {
///     match &event.data {
///         EventData::Http(http_event) => {
///             println!("Handling HTTP event");
///             // Process HTTP-specific event data
///         },
///         EventData::Runtime(runtime_event) => {
///             println!("Handling Runtime event");
///             // Process Runtime-specific event data
///         },
///         // Handle other event types
///         _ => println!("Other event type: {}", event.event_type),
///     }
/// }
/// ```
///
/// ## Implementation Notes
///
/// New event types should be added as variants to this enum when new subsystems
/// are implemented. Each variant should contain a properly defined data structure
/// that represents the specific event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventData {
    /// Environment variable access events for reading host environment variables.
    Environment(environment::EnvironmentEventData),
    /// File system access events, such as reading or writing files.
    Filesystem(filesystem::FilesystemEventData),
    /// HTTP-related events, including requests, responses, and WebSocket interactions.
    Http(http::HttpEventData),
    /// Actor-to-actor messaging events for communication between actors.
    Message(message::MessageEventData),
    /// OS Process management events, such as spawning processes and I/O.
    Process(process::ProcessEventData),
    /// Runtime lifecycle events, such as initialization, state changes, and shutdown.
    Runtime(runtime::RuntimeEventData),
    /// Supervision events related to actor parent-child relationships.
    Supervisor(supervisor::SupervisorEventData),
    /// Content store access events for the key-value storage system.
    Store(store::StoreEventData),
    /// Timer and scheduling events for time-based operations.
    Timing(timing::TimingEventData),
    /// WebAssembly execution events related to the WASM VM.
    Wasm(wasm::WasmEventData),
    /// Theater runtime system events for the global runtime coordination.
    TheaterRuntime(theater_runtime::TheaterRuntimeEventData),
}

impl ChainEventData {
    /// Gets the event type identifier string.
    ///
    /// ## Purpose
    ///
    /// This method returns the event type string, which can be used for filtering,
    /// routing, or categorizing events.
    ///
    /// ## Returns
    ///
    /// The event type as a String
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::events::ChainEventData;
    /// # use theater::events::EventData;
    /// # let event = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(runtime::RuntimeEventData::Init { params: vec![] }),
    /// #     timestamp: 0,
    /// #     description: None,
    /// # };
    ///
    /// let event_type = event.event_type();
    /// println!("Event type: {}", event_type);
    ///
    /// // Filter events based on type
    /// if event_type.starts_with("runtime.") {
    ///     println!("This is a runtime event");
    /// }
    /// ```
    #[allow(dead_code)]
    fn event_type(&self) -> String {
        let event_type = self.event_type.clone();
        event_type
    }

    /// Gets the human-readable description of the event, if available.
    ///
    /// ## Purpose
    ///
    /// This method returns the optional human-readable description of the event,
    /// which can be used for logging, debugging, or displaying to users. If no
    /// description is available, it returns an empty string.
    ///
    /// ## Returns
    ///
    /// The event description as a String, or an empty string if none is available
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::events::ChainEventData;
    /// # use theater::events::EventData;
    /// # let event = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(runtime::RuntimeEventData::Init { params: vec![] }),
    /// #     timestamp: 0,
    /// #     description: Some("Actor initialized".to_string()),
    /// # };
    ///
    /// let description = event.description();
    /// println!("Event description: {}", description);
    /// ```
    #[allow(dead_code)]
    fn description(&self) -> String {
        match &self.description {
            Some(desc) => desc.clone(),
            None => String::from(""),
        }
    }

    /// Serializes the event data to JSON.
    ///
    /// ## Purpose
    ///
    /// This method converts the event data to a JSON byte array, which can be used
    /// for storage, transmission over the network, or inclusion in a chain event.
    ///
    /// ## Returns
    ///
    /// * `Ok(Vec<u8>)` - The serialized JSON data
    /// * `Err(serde_json::Error)` - If serialization fails
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::events::ChainEventData;
    /// # use theater::events::EventData;
    /// # let event = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(runtime::RuntimeEventData::Init { params: vec![] }),
    /// #     timestamp: 0,
    /// #     description: None,
    /// # };
    ///
    /// match event.to_json() {
    ///     Ok(json_bytes) => {
    ///         println!("Serialized event: {} bytes", json_bytes.len());
    ///         // Store or transmit the JSON
    ///     },
    ///     Err(e) => println!("Failed to serialize event: {}", e),
    /// }
    /// ```
    #[allow(dead_code)]
    fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Converts the typed event data to a generic chain event.
    ///
    /// ## Purpose
    ///
    /// This method creates a `ChainEvent` from the typed event data, which is used
    /// to add the event to an actor's event chain. It serializes the event data and
    /// includes it in the chain event, along with metadata like the event type and
    /// timestamp.
    ///
    /// ## Parameters
    ///
    /// * `parent_hash` - Optional hash of the parent event in the chain
    ///
    /// ## Returns
    ///
    /// A new `ChainEvent` with the serialized event data and metadata
    ///
    /// ## Example
    ///
    /// ```rust
    /// # use theater::events::ChainEventData;
    /// # use theater::events::EventData;
    /// # use theater::chain::ChainEvent;
    /// # let event_data = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(runtime::RuntimeEventData::Init { params: vec![] }),
    /// #     timestamp: 0,
    /// #     description: None,
    /// # };
    ///
    /// // Convert to a chain event with no parent (root event)
    /// let chain_event = event_data.to_chain_event(None);
    ///
    /// // Later, create child events in the chain
    /// let child_event_data = /* create new event data */;
    /// // let child_chain_event = child_event_data.to_chain_event(Some(chain_event.hash.clone()));
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// The resulting `ChainEvent` will have an empty hash, which is filled in by the
    /// `StateChain` when the event is added to the chain. The hash is calculated based
    /// on the event content and cannot be known until the event is fully formed.
    pub fn to_chain_event(&self, parent_hash: Option<Vec<u8>>) -> ChainEvent {
        ChainEvent {
            parent_hash,
            hash: vec![],
            event_type: self.event_type.clone(),
            data: serde_json::to_vec(&self.data).unwrap_or_else(|_| vec![]),
            timestamp: self.timestamp,
            description: self.description.clone(),
        }
    }
}

pub mod environment;
pub mod filesystem;
pub mod http;
pub mod message;
pub mod process;
pub mod runtime;
pub mod store;
pub mod supervisor;
pub mod theater_runtime;
pub mod timing;
pub mod wasm;
