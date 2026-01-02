//! # Actor Store
//!
//! This module provides an abstraction for sharing resources between an actor and the Theater runtime system.
//! The ActorStore serves as a container for state, event chains, and communication channels that are
//! needed for WASM host functions to interface with the Actor system.

use crate::actor::handle::ActorHandle;
use crate::chain::{ChainEvent, StateChain};
use crate::events::{ChainEventData, EventPayload};
use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc::Sender;
use wasmtime::component::ResourceTable;

/// # ActorStore
///
/// A container for sharing actor resources with WebAssembly host functions.
///
/// The ActorStore serves as a central repository for all resources related to a specific actor instance.
/// It provides access to:
/// - The actor's unique identifier
/// - Communication channels to the Theater runtime
/// - The event chain for audit and verification
/// - The actor's current state data
/// - A handle to interact with the actor
/// - A resource table for Component Model resources (pollables, file handles, etc.)
#[derive(Clone)]
pub struct ActorStore<E>
where
    E: EventPayload + Clone,
{
    /// Unique identifier for the actor
    pub id: TheaterId,

    /// Channel for sending commands to the Theater runtime
    pub theater_tx: Sender<TheaterCommand>,

    /// The event chain that records all actor operations for verification and audit
    pub chain: Arc<RwLock<StateChain<E>>>,

    /// The current state of the actor, stored as a binary blob
    pub state: Option<Vec<u8>>,

    /// Handle to interact with the actor
    pub actor_handle: ActorHandle,

    /// Resource table for managing Component Model resources
    /// This table stores all resources (pollables, file handles, etc.) that are exposed to the actor
    pub resource_table: Arc<Mutex<ResourceTable>>,

    /// Extension storage for handlers to store arbitrary data
    /// Keyed by TypeId of the data type for type-safe retrieval
    /// This allows handlers to pass data from setup_host_functions to Host trait implementations
    pub extensions: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl<E> ActorStore<E>
where
    E: EventPayload + Clone,
{
    /// # Create a new ActorStore
    ///
    /// Creates a new instance of the ActorStore with the given parameters.
    ///
    /// ## Parameters
    ///
    /// * `id` - Unique identifier for the actor
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `message_tx` - Optional channel for sending message commands to the message-server handler
    /// * `actor_handle` - Handle for interacting with the actor
    ///
    /// ## Returns
    ///
    /// A new ActorStore instance configured with the provided parameters.
    pub fn new(
        id: TheaterId,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
        chain: Arc<RwLock<StateChain<E>>>,
    ) -> Self {
        Self {
            id: id.clone(),
            theater_tx: theater_tx.clone(),
            chain,
            state: Some(vec![]),
            actor_handle,
            resource_table: Arc::new(Mutex::new(ResourceTable::new())),
            extensions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// # Get the actor's ID
    ///
    /// Retrieves the unique identifier for this actor.
    ///
    /// ## Returns
    ///
    /// A clone of the actor's TheaterId.
    pub fn get_id(&self) -> TheaterId {
        self.id.clone()
    }

    /// # Get the Theater command channel
    ///
    /// Retrieves the channel used for sending commands to the Theater runtime.
    ///
    /// ## Returns
    ///
    /// A clone of the Sender<TheaterCommand> channel.
    pub fn get_theater_tx(&self) -> Sender<TheaterCommand> {
        self.theater_tx.clone()
    }

    /// # Get the actor's state
    ///
    /// Retrieves the current state data for this actor.
    ///
    /// ## Returns
    ///
    /// A clone of the actor's state as an Option<Vec<u8>>.
    /// - Some(Vec<u8>) if state exists
    /// - None if no state has been set
    pub fn get_state(&self) -> Option<Vec<u8>> {
        self.state.clone()
    }

    /// # Set the actor's state
    ///
    /// Updates the current state data for this actor.
    ///
    /// ## Parameters
    ///
    /// * `state` - The new state data as an Option<Vec<u8>>
    ///   - Some(Vec<u8>) to set a specific state
    ///   - None to clear the state
    pub fn set_state(&mut self, state: Option<Vec<u8>>) {
        self.state = state;
    }

    /// # Record an event in the chain
    ///
    /// Adds a new event to the actor's event chain.
    ///
    /// ## Parameters
    ///
    /// * `event_data` - The event data to record, typically a variant of ChainEventData
    ///
    /// ## Returns
    ///
    /// The ChainEvent that was created and added to the chain.
    pub fn record_event(&self, event_data: ChainEventData<E>) -> ChainEvent {
        let mut chain = self.chain.write().unwrap();
        chain
            .add_typed_event(event_data)
            .expect("Failed to record event")
    }

    /// # Record a handler-specific event with type-safe conversion
    ///
    /// This method allows handlers to record their events while maintaining type safety.
    /// The application must implement `From<H>` for its event type E, which the compiler
    /// enforces. This ensures that applications can only use handlers for which they've
    /// implemented proper event conversion.
    ///
    /// ## Type Parameters
    ///
    /// * `H` - The handler-specific event type. The application's event type `E` must
    ///   implement `From<H>`, which is checked at compile time.
    ///
    /// ## Parameters
    ///
    /// * `event_type` - The type identifier for this event (e.g., "theater:simple/environment/get-var")
    /// * `handler_event` - The handler-specific event data
    /// * `description` - Optional human-readable description
    ///
    /// ## Returns
    ///
    /// The ChainEvent that was created and added to the chain.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // In a handler:
    /// ctx.data_mut().record_handler_event(
    ///     "theater:simple/environment/get-var".to_string(),
    ///     EnvironmentEventData::GetVar {
    ///         variable_name: var_name.clone(),
    ///         success: true,
    ///         value_found,
    ///         timestamp: Utc::now(),
    ///     },
    ///     Some(format!("Environment variable access: {}", var_name)),
    /// );
    /// ```
    ///
    /// ## Compile-Time Safety
    ///
    /// If the application hasn't implemented `From<EnvironmentEventData>` for its event type,
    /// this call will fail to compile with a clear error message indicating the missing trait.
    pub fn record_handler_event<H>(
        &self,
        event_type: String,
        handler_event: H,
        description: Option<String>,
    ) -> ChainEvent
    where
        E: From<H>,
        H: serde::Serialize + Clone,
    {
        // Convert handler event to application event type using From trait
        let app_event: E = handler_event.into();

        self.record_event(ChainEventData {
            event_type,
            data: app_event,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description,
        })
    }

    /// Record a core runtime event with type-safe conversion
    ///
    /// This method allows the core runtime to record Runtime events.
    pub fn record_runtime_event<R>(
        &self,
        event_type: String,
        runtime_event: R,
        description: Option<String>,
    ) -> ChainEvent
    where
        E: From<R>,
        R: serde::Serialize + Clone,
    {
        let app_event: E = runtime_event.into();
        self.record_event(ChainEventData {
            event_type,
            data: app_event,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description,
        })
    }

    /// Record a core WASM event with type-safe conversion
    pub fn record_wasm_event<W>(
        &self,
        event_type: String,
        wasm_event: W,
        description: Option<String>,
    ) -> ChainEvent
    where
        E: From<W>,
        W: serde::Serialize + Clone,
    {
        let app_event: E = wasm_event.into();
        self.record_event(ChainEventData {
            event_type,
            data: app_event,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description,
        })
    }

    /// Record a core TheaterRuntime event with type-safe conversion
    pub fn record_theater_runtime_event<T>(
        &self,
        event_type: String,
        theater_event: T,
        description: Option<String>,
    ) -> ChainEvent
    where
        E: From<T>,
        T: serde::Serialize + Clone,
    {
        let app_event: E = theater_event.into();
        self.record_event(ChainEventData {
            event_type,
            data: app_event,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description,
        })
    }

    /// Record a host function call with full I/O for replay.
    ///
    /// This is the standardized way to record handler host function calls.
    /// The event captures the interface name, function name, and serialized
    /// input/output, which is everything needed to replay the call.
    ///
    /// ## Parameters
    ///
    /// * `interface` - The WIT interface name (e.g., "theater:simple/timing")
    /// * `function` - The function name (e.g., "now")
    /// * `input` - Serialized input parameters
    /// * `output` - Serialized output/return value
    /// * `description` - Optional human-readable description
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // Record a timing "now" call
    /// ctx.data_mut().record_host_function_call(
    ///     "theater:simple/timing",
    ///     "now",
    ///     &(),           // no input
    ///     &timestamp,    // output
    ///     Some(format!("now() -> {}", timestamp)),
    /// );
    /// ```
    pub fn record_host_function_call<I, O>(
        &self,
        interface: &str,
        function: &str,
        input: &I,
        output: &O,
        description: Option<String>,
    ) -> ChainEvent
    where
        E: From<crate::replay::HostFunctionCall>,
        I: serde::Serialize,
        O: serde::Serialize,
    {
        let host_call = crate::replay::HostFunctionCall {
            interface: interface.to_string(),
            function: function.to_string(),
            input: serde_json::to_vec(input).unwrap_or_default(),
            output: serde_json::to_vec(output).unwrap_or_default(),
        };

        let app_event: E = host_call.into();
        self.record_event(ChainEventData {
            event_type: format!("{}/{}", interface, function),
            data: app_event,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description,
        })
    }

    /// # Verify the integrity of the event chain
    ///
    /// Checks that the event chain has not been tampered with.
    ///
    /// ## Returns
    ///
    /// A boolean indicating whether the chain is valid:
    /// - `true` if the chain integrity is verified
    /// - `false` if any tampering or inconsistency is detected
    pub fn verify_chain(&self) -> bool {
        let chain = self.chain.read().unwrap();
        chain.verify()
    }

    /// # Get the most recent event
    ///
    /// Retrieves the last event that was added to the chain.
    ///
    /// ## Returns
    ///
    /// - `Some(ChainEvent)` with the most recent event
    /// - `None` if the chain is empty
    pub fn get_last_event(&self) -> Option<ChainEvent> {
        let chain = self.chain.read().unwrap();
        chain.get_last_event().cloned()
    }

    /// # Get all events in the chain
    ///
    /// Retrieves all events that have been recorded in the chain.
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> containing all events in chronological order.
    pub fn get_all_events(&self) -> Vec<ChainEvent> {
        let chain = self.chain.read().unwrap();
        chain.get_events().to_vec()
    }

    /// # Get the event chain
    ///
    /// Alias for get_all_events(), retrieves all events in the chain.
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> containing all events in chronological order.
    pub fn get_chain(&self) -> Vec<ChainEvent> {
        self.get_all_events()
    }

    /// # Save the event chain to a file
    ///
    /// Persists the entire event chain to a file on disk.
    ///
    /// ## Parameters
    ///
    /// * `path` - The file path where the chain should be saved
    ///
    /// ## Returns
    ///
    /// - `Ok(())` if the chain was successfully saved
    /// - `Err(anyhow::Error)` if an error occurred during saving
    pub fn save_chain(&self) -> anyhow::Result<()> {
        let chain = self.chain.read().unwrap();
        chain.save_chain()?;
        Ok(())
    }

    pub fn get_actor_handle(&self) -> ActorHandle {
        self.actor_handle.clone()
    }

    // =========================================================================
    // Extension Methods
    // =========================================================================

    /// Store extension data of a specific type
    ///
    /// Handlers use this to store data during setup that can be retrieved
    /// later in Host trait implementations. Each type can only have one value;
    /// calling this again with the same type will overwrite the previous value.
    ///
    /// ## Type Parameters
    ///
    /// * `T` - The type of data to store. Must be Send + Sync + 'static.
    ///
    /// ## Parameters
    ///
    /// * `value` - The value to store
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// #[derive(Clone)]
    /// struct MyHandlerConfig { path: PathBuf }
    ///
    /// // In setup_host_functions:
    /// actor_store.set_extension(MyHandlerConfig { path: "/tmp".into() });
    /// ```
    pub fn set_extension<T: Send + Sync + 'static>(&self, value: T) {
        let mut extensions = self.extensions.write().unwrap();
        extensions.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Retrieve extension data of a specific type
    ///
    /// Returns a clone of the stored value if it exists and matches the requested type.
    ///
    /// ## Type Parameters
    ///
    /// * `T` - The type of data to retrieve. Must be Clone + Send + Sync + 'static.
    ///
    /// ## Returns
    ///
    /// * `Some(T)` - A clone of the stored value
    /// * `None` - If no value of this type was stored
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // In Host trait implementation:
    /// if let Some(config) = self.get_extension::<MyHandlerConfig>() {
    ///     println!("Using path: {:?}", config.path);
    /// }
    /// ```
    pub fn get_extension<T: Clone + Send + Sync + 'static>(&self) -> Option<T> {
        let extensions = self.extensions.read().unwrap();
        extensions
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
            .cloned()
    }

    /// Check if extension data of a specific type exists
    ///
    /// ## Type Parameters
    ///
    /// * `T` - The type to check for
    ///
    /// ## Returns
    ///
    /// `true` if a value of this type is stored, `false` otherwise
    pub fn has_extension<T: Send + Sync + 'static>(&self) -> bool {
        let extensions = self.extensions.read().unwrap();
        extensions.contains_key(&TypeId::of::<T>())
    }

    /// Remove and return extension data of a specific type
    ///
    /// ## Type Parameters
    ///
    /// * `T` - The type of data to remove
    ///
    /// ## Returns
    ///
    /// * `Some(T)` - The removed value
    /// * `None` - If no value of this type was stored
    pub fn remove_extension<T: Send + Sync + 'static>(&self) -> Option<T> {
        let mut extensions = self.extensions.write().unwrap();
        extensions
            .remove(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast::<T>().ok())
            .map(|b| *b)
    }

    // =========================================================================
    // Event Query Methods
    // =========================================================================
    // These benefit significantly from RwLock's concurrent read access

    /// # Get events by type
    ///
    /// Filters events by their event_type field.
    /// Multiple callers can execute this concurrently without blocking.
    ///
    /// ## Parameters
    ///
    /// * `event_type` - The event type to filter by
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> containing only events of the specified type.
    pub fn get_events_by_type(&self, event_type: &str) -> Vec<ChainEvent> {
        let chain = self.chain.read().unwrap();
        chain
            .get_events()
            .iter()
            .filter(|e| e.event_type == event_type)
            .cloned()
            .collect()
    }

    /// # Get recent events
    ///
    /// Gets the most recent N events from the chain.
    /// Useful for monitoring and health checks.
    ///
    /// ## Parameters
    ///
    /// * `count` - Maximum number of recent events to return
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> with up to `count` most recent events.
    pub fn get_recent_events(&self, count: usize) -> Vec<ChainEvent> {
        let chain = self.chain.read().unwrap();
        let events = chain.get_events();
        events.iter().rev().take(count).cloned().collect()
    }

    /// # Get events since timestamp
    ///
    /// Gets all events that occurred after the specified timestamp.
    /// Perfect for incremental monitoring.
    ///
    /// ## Parameters
    ///
    /// * `since` - Unix timestamp to filter events after
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> containing events after the timestamp.
    pub fn get_events_since(&self, since: u64) -> Vec<ChainEvent> {
        let chain = self.chain.read().unwrap();
        chain
            .get_events()
            .iter()
            .filter(|e| e.timestamp > since)
            .cloned()
            .collect()
    }

    /// # Check if chain contains event type
    ///
    /// Quick check to see if the chain contains any events of a specific type.
    /// More efficient than filtering all events when you just need existence.
    ///
    /// ## Parameters
    ///
    /// * `event_type` - The event type to search for
    ///
    /// ## Returns
    ///
    /// true if at least one event of this type exists, false otherwise.
    pub fn has_event_type(&self, event_type: &str) -> bool {
        let chain = self.chain.read().unwrap();
        chain
            .get_events()
            .iter()
            .any(|e| e.event_type == event_type)
    }
}
