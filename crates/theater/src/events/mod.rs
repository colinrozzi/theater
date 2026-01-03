use crate::chain::ChainEvent;
use crate::replay::HostFunctionCall;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Trait implemented by any payload type that can be recorded in the Theater
/// event chain. External handler crates can implement this to integrate their
/// custom event enums with the runtime.
pub trait EventPayload:
    Serialize + for<'de> Deserialize<'de> + Send + Sync + Debug + 'static
{
}

impl<T> EventPayload for T where
    T: Serialize + for<'de> Deserialize<'de> + Send + Sync + Debug + 'static
{
}

/// # Theater Events Wrapper
///
/// `TheaterEvents<H>` is a wrapper enum that combines core Theater runtime events
/// with handler-specific events. This allows applications to define only the handler
/// events they need while ensuring core runtime events are always available.
///
/// ## Purpose
///
/// This enum provides a type-safe way to compose events from different handlers
/// while maintaining compile-time safety. Applications define their own handler
/// event enum `H` and the type system enforces that all handlers implement
/// proper event conversion.
///
/// ## Type Parameters
///
/// * `H` - The application's handler event enum type, which must be serializable,
///   cloneable, and implement Send + Sync for thread safety.
///
/// ## Example
///
/// ```rust
/// use theater::events::TheaterEvents;
/// use theater_handler_environment::EnvironmentEventData;
/// use serde::{Deserialize, Serialize};
///
/// // Define your application's handler events
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// pub enum MyHandlerEvents {
///     Environment(EnvironmentEventData),
///     // ... other handlers you use
/// }
///
/// // Your application's complete event type
/// pub type MyAppEvents = TheaterEvents<MyHandlerEvents>;
///
/// // Implement From trait for type-safe event recording
/// impl From<EnvironmentEventData> for MyAppEvents {
///     fn from(event: EnvironmentEventData) -> Self {
///         TheaterEvents::Handler(MyHandlerEvents::Environment(event))
///     }
/// }
/// ```
///
/// ## Core Event Categories
///
/// - **Runtime**: Actor lifecycle events (init, shutdown, state changes, logs, errors)
/// - **Wasm**: WebAssembly execution events (component creation, function calls)
/// - **TheaterRuntime**: System-level events (actor loading, permissions, updates)
/// - **HostFunction**: Standardized host function call recording (for replay)
/// - **Handler**: Application-specific handler events (defined by application)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "category")]
#[serde(bound(
    serialize = "H: Serialize",
    deserialize = "H: serde::de::DeserializeOwned"
))]
pub enum TheaterEvents<H>
where
    H: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync,
{
    /// Core actor runtime events (lifecycle, init, shutdown, state, logs)
    Runtime(runtime::RuntimeEventData),

    /// Core WASM execution events (component creation, function calls)
    Wasm(wasm::WasmEventData),

    /// Core theater system events (actor loading, permissions, updates)
    TheaterRuntime(theater_runtime::TheaterRuntimeEventData),

    /// Standardized host function call with full I/O (for replay)
    HostFunction(HostFunctionCall),

    /// Handler-specific events (environment, HTTP, timing, etc.)
    Handler(H),
}

impl<H> TheaterEvents<H>
where
    H: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync,
{
    /// Creates a Runtime event variant
    pub fn runtime(event: runtime::RuntimeEventData) -> Self {
        TheaterEvents::Runtime(event)
    }

    /// Creates a Wasm event variant
    pub fn wasm(event: wasm::WasmEventData) -> Self {
        TheaterEvents::Wasm(event)
    }

    /// Creates a TheaterRuntime event variant
    pub fn theater_runtime(event: theater_runtime::TheaterRuntimeEventData) -> Self {
        TheaterEvents::TheaterRuntime(event)
    }

    /// Creates a Handler event variant
    pub fn handler(event: H) -> Self {
        TheaterEvents::Handler(event)
    }

    /// Creates a HostFunction event variant
    pub fn host_function(event: HostFunctionCall) -> Self {
        TheaterEvents::HostFunction(event)
    }
}

// Implement From for core event types so they can be automatically converted
impl<H> From<runtime::RuntimeEventData> for TheaterEvents<H>
where
    H: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync,
{
    fn from(event: runtime::RuntimeEventData) -> Self {
        TheaterEvents::Runtime(event)
    }
}

impl<H> From<wasm::WasmEventData> for TheaterEvents<H>
where
    H: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync,
{
    fn from(event: wasm::WasmEventData) -> Self {
        TheaterEvents::Wasm(event)
    }
}

impl<H> From<theater_runtime::TheaterRuntimeEventData> for TheaterEvents<H>
where
    H: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync,
{
    fn from(event: theater_runtime::TheaterRuntimeEventData) -> Self {
        TheaterEvents::TheaterRuntime(event)
    }
}

impl<H> From<HostFunctionCall> for TheaterEvents<H>
where
    H: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync,
{
    fn from(event: HostFunctionCall) -> Self {
        TheaterEvents::HostFunction(event)
    }
}

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
///     data: EventData::Runtime(RuntimeEventData::InitCall {
///         params: "init params".to_string(),
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
#[serde(bound(
    serialize = "E: Serialize",
    deserialize = "E: serde::de::DeserializeOwned"
))]
pub struct ChainEventData<E>
where
    E: EventPayload,
{
    /// The type identifier for this event, used for filtering and routing.
    /// This should be a dot-separated string like "subsystem.action".
    pub event_type: String,
    /// The specific event data payload, containing domain-specific information.
    pub data: E,
}

impl<E> ChainEventData<E>
where
    E: EventPayload,
{
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
    /// # use theater::events::runtime::RuntimeEventData;
    /// # let event = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(RuntimeEventData::InitCall { params: String::new() }),
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
    pub fn event_type(&self) -> String {
        let event_type = self.event_type.clone();
        event_type
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
    /// # use theater::events::runtime::RuntimeEventData;
    /// # let event = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(RuntimeEventData::InitCall { params: String::new() }),
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
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
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
    /// # use theater::events::runtime::RuntimeEventData;
    /// # use theater::chain::ChainEvent;
    /// # let event_data = ChainEventData {
    /// #     event_type: "runtime.init".to_string(),
    /// #     data: EventData::Runtime(RuntimeEventData::InitCall { params: String::new() }),
    /// #     description: None,
    /// # };
    ///
    /// // Convert to a chain event with no parent (root event)
    /// let chain_event = event_data.to_chain_event(None);
    ///
    /// // Later, create child events in the chain
    /// // Create a child event
    /// let child_event_data = ChainEventData {
    ///     event_type: "child.event".to_string(),
    ///     data: EventData::Runtime(RuntimeEventData::Log { level: "info".to_string(), message: "child event".to_string() }),
    ///     description: None,
    /// };
    /// let child_chain_event = child_event_data.to_chain_event(Some(chain_event.hash.clone()));
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
        }
    }
}

/// Default event type for when an application doesn't define custom handler events.
/// This type uses `()` for the handler events variant, meaning no custom handler events are used.
pub type EventData = TheaterEvents<()>;

pub mod runtime;
pub mod theater_runtime;
pub mod wasm;
