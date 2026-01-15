//! # Actor Store
//!
//! This module provides an abstraction for sharing resources between an actor and the Theater runtime system.
//! The ActorStore serves as a container for state, event chains, and communication channels that are
//! needed for WASM host functions to interface with the Actor system.

use crate::actor::handle::ActorHandle;
use crate::chain::{ChainEvent, StateChain};
use crate::events::{ChainEventData, ChainEventPayload};
use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::replay::HostFunctionCall;
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
pub struct ActorStore {
    /// Unique identifier for the actor
    pub id: TheaterId,

    /// Channel for sending commands to the Theater runtime
    pub theater_tx: Sender<TheaterCommand>,

    /// The event chain that records all actor operations for verification and audit
    pub chain: Arc<RwLock<StateChain>>,

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

impl ActorStore {
    /// # Create a new ActorStore
    ///
    /// Creates a new instance of the ActorStore with the given parameters.
    ///
    /// ## Parameters
    ///
    /// * `id` - Unique identifier for the actor
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `actor_handle` - Handle for interacting with the actor
    /// * `chain` - The event chain for this actor
    ///
    /// ## Returns
    ///
    /// A new ActorStore instance configured with the provided parameters.
    pub fn new(
        id: TheaterId,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
        chain: Arc<RwLock<StateChain>>,
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
    /// * `event_data` - The event data to record
    ///
    /// ## Returns
    ///
    /// The ChainEvent that was created and added to the chain.
    pub fn record_event(&self, event_data: ChainEventData) -> ChainEvent {
        let mut chain = self.chain.write().unwrap();
        chain
            .add_typed_event(event_data)
            .expect("Failed to record event")
    }

    /// Record a host function call with full I/O for replay.
    ///
    /// This is the standardized way to record handler host function calls.
    /// The event captures the interface name, function name, and typed
    /// input/output values, which is everything needed to replay the call.
    ///
    /// ## Parameters
    ///
    /// * `interface` - The WIT interface name (e.g., "wasi:clocks/wall-clock@0.2.3")
    /// * `function` - The function name (e.g., "now")
    /// * `input` - Input parameters as SerializableVal (use `.into_serializable_val()`)
    /// * `output` - Output/return value as SerializableVal (use `.into_serializable_val()`)
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// use val_serde::IntoSerializableVal;
    ///
    /// // Record a random "get-random-u64" call
    /// ctx.data_mut().record_host_function_call(
    ///     "wasi:random/random@0.2.3",
    ///     "get-random-u64",
    ///     ().into_serializable_val(),           // no input
    ///     value.into_serializable_val(),        // output
    /// );
    /// ```
    pub fn record_host_function_call(
        &self,
        interface: &str,
        function: &str,
        input: val_serde::SerializableVal,
        output: val_serde::SerializableVal,
    ) -> ChainEvent {
        tracing::debug!(
            "[RECORD] Host function call: {}/{}",
            interface,
            function
        );

        let host_call = HostFunctionCall::new(interface, function, input, output);

        self.record_event(ChainEventData {
            event_type: format!("{}/{}", interface, function),
            data: ChainEventPayload::HostFunction(host_call),
        })
    }

    /// Record a WebAssembly execution event.
    ///
    /// This is used for recording WASM function calls, results, and errors.
    ///
    /// ## Parameters
    ///
    /// * `event_type` - A string identifier for this event (e.g., "wasm-call")
    /// * `data` - The WasmEventData containing the event details
    pub fn record_wasm_event(
        &self,
        event_type: String,
        data: crate::events::wasm::WasmEventData,
    ) -> ChainEvent {
        self.record_event(ChainEventData {
            event_type,
            data: ChainEventPayload::Wasm(data),
        })
    }

    /// Record a theater runtime event (for debugging/audit purposes).
    ///
    /// Note: These events are recorded as Wasm events with a special event type
    /// since they're primarily for debugging and not essential for replay.
    ///
    /// ## Parameters
    ///
    /// * `event_type` - A string identifier for this event
    /// * `data` - The TheaterRuntimeEventData
    pub fn record_theater_runtime_event(
        &self,
        event_type: String,
        data: crate::events::theater_runtime::TheaterRuntimeEventData,
    ) -> ChainEvent {
        // Convert theater runtime events to Wasm events for storage
        // These are primarily for debugging/audit and not essential for replay
        let wasm_data = crate::events::wasm::WasmEventData::WasmCall {
            function_name: event_type.clone(),
            params: serde_json::to_vec(&data).unwrap_or_default(),
        };
        self.record_event(ChainEventData {
            event_type: format!("theater-runtime/{}", event_type),
            data: ChainEventPayload::Wasm(wasm_data),
        })
    }

    /// DEPRECATED: Legacy method for backward compatibility with handlers.
    ///
    /// This method is a no-op stub that allows existing handler code to compile.
    /// Handlers should migrate to using `record_host_function_call` instead.
    ///
    /// TODO: Remove this once all handlers are updated to use the new event system.
    pub fn record_handler_event<T: serde::Serialize>(
        &self,
        _event_type: String,
        _data: T,
        _description: Option<String>,
    ) {
        // No-op for backwards compatibility
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
