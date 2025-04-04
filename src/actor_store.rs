
//! # Actor Store
//!
//! This module provides an abstraction for sharing resources between an actor and the Theater runtime system.
//! The ActorStore serves as a container for state, event chains, and communication channels that are 
//! needed for WASM host functions to interface with the Actor system.
//!
//! ## Purpose
//!
//! The ActorStore provides a centralized location for storing and managing resources that need to be shared 
//! across different parts of the Theater system, particularly between the main runtime and the WebAssembly 
//! host functions. It maintains:
//!
//! - The actor's persistent state
//! - The event chain for auditing and verification
//! - Communication channels to the main Theater runtime
//! - Actor identity and references
//!
//! ## Example
//!
//! ```rust
//! use theater::actor_store::ActorStore;
//! use theater::id::TheaterId;
//! use theater::messages::TheaterCommand;
//! use theater::actor_handle::ActorHandle;
//! use tokio::sync::mpsc;
//!
//! async fn example_usage() {
//!     // Create a channel for Theater commands
//!     let (tx, mut rx) = mpsc::channel(100);
//!     
//!     // Create an actor ID
//!     let id = TheaterId::generate();
//!     
//!     // Create an actor handle (simplified for example)
//!     let handle = ActorHandle::new(id.clone(), tx.clone());
//!     
//!     // Create the actor store
//!     let store = ActorStore::new(id, tx, handle);
//!     
//!     // Use the store to save state
//!     let mut store_clone = store.clone();
//!     store_clone.set_state(Some(b"Hello, Theater!".to_vec()));
//!     
//!     // Use the store to record events
//!     use theater::events::ChainEventData;
//!     let event = store.record_event(ChainEventData::StateUpdated {
//!         previous_hash: vec![],
//!         new_state: b"Updated state".to_vec(),
//!     });
//! }
//! ```
//!
//! ## Security
//!
//! The ActorStore is a critical component in Theater's security model. It:
//!
//! - Maintains the integrity of the event chain through cryptographic verification
//! - Provides controlled access to the actor's state
//! - Creates a boundary between the WebAssembly execution environment and the host system
//!
//! ## Implementation Notes
//!
//! The ActorStore uses thread-safe containers (Arc<Mutex<>>) to ensure safe concurrent access
//! to shared resources. It clones data when necessary to avoid shared mutable state. The event
//! chain is a critical data structure for tracking and auditing actor behavior, and the store
//! provides methods to manipulate and verify it.

use crate::actor_handle::ActorHandle;
use crate::chain::{ChainEvent, StateChain};
use crate::events::ChainEventData;
use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

/// # ActorStore
///
/// A container for sharing actor resources with WebAssembly host functions.
///
/// ## Purpose
///
/// The ActorStore serves as a central repository for all resources related to a specific actor instance.
/// It provides access to:
/// - The actor's unique identifier
/// - Communication channels to the Theater runtime
/// - The event chain for audit and verification
/// - The actor's current state data
/// - A handle to interact with the actor
///
/// These resources are made available to WebAssembly host functions when executing
/// actor component code, allowing controlled access to Theater system functionality.
///
/// ## Example
///
/// ```rust
/// use theater::actor_store::ActorStore;
/// use theater::id::TheaterId;
/// use tokio::sync::mpsc;
///
/// async fn create_store(actor_handle: ActorHandle) {
///     // Create a communication channel
///     let (tx, _rx) = mpsc::channel(32);
///     
///     // Generate a unique ID for the actor
///     let id = TheaterId::generate();
///     
///     // Create the actor store
///     let store = ActorStore::new(id, tx, actor_handle);
///     
///     // The store can now be used to record events and manage state
/// }
/// ```
///
/// ## Security
///
/// The ActorStore is designed to expose only the necessary functionality to
/// WebAssembly components, creating a security boundary between the host and
/// guest environments. All interaction with the event chain and state is mediated
/// through the store, ensuring proper validation and integrity checking.
///
/// ## Implementation Notes
///
/// The ActorStore uses thread-safe containers (Arc<Mutex<>>) to allow sharing
/// across thread boundaries, particularly important when used with the WebAssembly
/// host functions that may execute in different threads from the main runtime.
/// The `.clone()` method creates a new reference to the same underlying data.
#[derive(Clone)]
pub struct ActorStore {
    /// Unique identifier for the actor
    pub id: TheaterId,
    
    /// Channel for sending commands to the Theater runtime
    pub theater_tx: Sender<TheaterCommand>,
    
    /// The event chain that records all actor operations for verification and audit
    pub chain: Arc<Mutex<StateChain>>,
    
    /// The current state of the actor, stored as a binary blob
    pub state: Option<Vec<u8>>,
    
    /// Handle to interact with the actor
    pub actor_handle: ActorHandle,
}

impl ActorStore {
    /// # Create a new ActorStore
    ///
    /// Creates a new instance of the ActorStore with the given parameters.
    ///
    /// ## Purpose
    ///
    /// This function initializes a new ActorStore with all the necessary resources
    /// required for an actor to function within the Theater system.
    ///
    /// ## Parameters
    ///
    /// * `id` - Unique identifier for the actor
    /// * `theater_tx` - Channel for sending commands to the Theater runtime
    /// * `actor_handle` - Handle for interacting with the actor
    ///
    /// ## Returns
    ///
    /// A new ActorStore instance configured with the provided parameters.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let store = ActorStore::new(actor_id, tx, handle);
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// The function initializes:
    /// - An empty state vector
    /// - A new StateChain linked to the actor's ID
    /// - Clones of the provided parameters to ensure ownership
    pub fn new(
        id: TheaterId,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
    ) -> Self {
        Self {
            id: id.clone(),
            theater_tx: theater_tx.clone(),
            chain: Arc::new(Mutex::new(StateChain::new(id, theater_tx))),
            state: Some(vec![]),
            actor_handle,
        }
    }

    /// # Get the actor's ID
    ///
    /// Retrieves the unique identifier for this actor.
    ///
    /// ## Returns
    ///
    /// A clone of the actor's TheaterId.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let actor_id = store.get_id();
    /// println!("Working with actor: {}", actor_id);
    /// ```
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
    ///
    /// ## Example
    ///
    /// ```rust
    /// let tx = store.get_theater_tx();
    /// tx.send(TheaterCommand::Ping).await.unwrap();
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This returns a clone of the channel, which is a relatively lightweight
    /// operation as Sender is internally an Arc.
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
    ///
    /// ## Example
    ///
    /// ```rust
    /// if let Some(state) = store.get_state() {
    ///     // Process the state data
    ///     let state_string = String::from_utf8_lossy(&state);
    ///     println!("Current state: {}", state_string);
    /// } else {
    ///     println!("No state available");
    /// }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This returns a clone of the state data to avoid sharing mutable references.
    /// For large state objects, consider more efficient state management strategies.
    pub fn get_state(&self) -> Option<Vec<u8>> {
        self.state.clone()
    }

    /// # Set the actor's state
    ///
    /// Updates the current state data for this actor.
    ///
    /// ## Purpose
    ///
    /// This method is used to update the actor's state, typically after processing
    /// operations that modify the actor's internal data.
    ///
    /// ## Parameters
    ///
    /// * `state` - The new state data as an Option<Vec<u8>>
    ///   - Some(Vec<u8>) to set a specific state
    ///   - None to clear the state
    ///
    /// ## Example
    ///
    /// ```rust
    /// // Set a new state
    /// let mut store_mut = store.clone();
    /// store_mut.set_state(Some(serde_json::to_vec(&my_state).unwrap()));
    ///
    /// // Clear the state
    /// store_mut.set_state(None);
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This method only updates the state in the ActorStore. For proper auditing
    /// and verification, any state changes should also be recorded in the event chain
    /// using the `record_event` method.
    pub fn set_state(&mut self, state: Option<Vec<u8>>) {
        self.state = state;
    }

    /// # Record an event in the chain
    ///
    /// Adds a new event to the actor's event chain.
    ///
    /// ## Purpose
    ///
    /// Records operations, state changes, and other significant events in the
    /// actor's audit chain for verification and traceability.
    ///
    /// ## Parameters
    ///
    /// * `event_data` - The event data to record, typically a variant of ChainEventData
    ///
    /// ## Returns
    ///
    /// The ChainEvent that was created and added to the chain.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::events::ChainEventData;
    ///
    /// // Record a state update event
    /// let event = store.record_event(ChainEventData::StateUpdated {
    ///     previous_hash: previous_hash.clone(),
    ///     new_state: new_state.clone(),
    /// });
    ///
    /// println!("Recorded event with hash: {:?}", event.hash);
    /// ```
    ///
    /// ## Security
    ///
    /// The event chain is a critical component of Theater's security and audit system.
    /// Each event is cryptographically linked to previous events, creating a tamper-evident
    /// record of all operations. This supports verification and replay of actor state.
    ///
    /// ## Implementation Notes
    ///
    /// This method acquires a lock on the chain, which can block if concurrent
    /// access is attempted. The chain ensures that events are properly linked
    /// and hashed for verification.
    pub fn record_event(&self, event_data: ChainEventData) -> ChainEvent {
        let mut chain = self.chain.lock().unwrap();
        chain
            .add_typed_event(event_data)
            .expect("Failed to record event")
    }

    /// # Verify the integrity of the event chain
    ///
    /// Checks that the event chain has not been tampered with.
    ///
    /// ## Purpose
    ///
    /// This method verifies the cryptographic integrity of the event chain,
    /// ensuring that all events are properly linked and that no events have
    /// been modified after recording.
    ///
    /// ## Returns
    ///
    /// A boolean indicating whether the chain is valid:
    /// - `true` if the chain integrity is verified
    /// - `false` if any tampering or inconsistency is detected
    ///
    /// ## Example
    ///
    /// ```rust
    /// if store.verify_chain() {
    ///     println!("Chain integrity verified");
    /// } else {
    ///     println!("Chain integrity verification failed!");
    ///     // Take appropriate action for security breach
    /// }
    /// ```
    ///
    /// ## Security
    ///
    /// Regular verification of the chain integrity is a key security practice.
    /// Applications should verify the chain at critical points, especially
    /// before making security decisions based on event history.
    ///
    /// ## Implementation Notes
    ///
    /// This method acquires a lock on the chain during verification.
    /// Verification includes checking the cryptographic links between
    /// events and validating the chain structure.
    pub fn verify_chain(&self) -> bool {
        let chain = self.chain.lock().unwrap();
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
    ///
    /// ## Example
    ///
    /// ```rust
    /// if let Some(event) = store.get_last_event() {
    ///     println!("Last event type: {:?}", event.data);
    ///     println!("Occurred at: {:?}", event.timestamp);
    /// } else {
    ///     println!("No events recorded yet");
    /// }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This method acquires a lock on the chain and returns a clone of the
    /// last event to avoid exposing internal mutability.
    pub fn get_last_event(&self) -> Option<ChainEvent> {
        let chain = self.chain.lock().unwrap();
        chain.get_last_event().cloned()
    }

    /// # Get all events in the chain
    ///
    /// Retrieves all events that have been recorded in the chain.
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> containing all events in chronological order.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let events = store.get_all_events();
    /// println!("Total events recorded: {}", events.len());
    ///
    /// for (i, event) in events.iter().enumerate() {
    ///     println!("Event {}: {:?} at {:?}", i, event.data, event.timestamp);
    /// }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This method acquires a lock on the chain and returns a copy of all events.
    /// For chains with many events, this can be memory-intensive. Consider using
    /// pagination or filtering if working with large chains.
    pub fn get_all_events(&self) -> Vec<ChainEvent> {
        let chain = self.chain.lock().unwrap();
        chain.get_events().to_vec()
    }

    /// # Get the event chain
    ///
    /// Alias for get_all_events(), retrieves all events in the chain.
    ///
    /// ## Returns
    ///
    /// A Vec<ChainEvent> containing all events in chronological order.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let chain = store.get_chain();
    /// for event in chain {
    ///     // Process each event
    /// }
    /// ```
    ///
    /// ## Implementation Notes
    ///
    /// This is an alias for `get_all_events()` and has the same performance
    /// characteristics and locking behavior.
    pub fn get_chain(&self) -> Vec<ChainEvent> {
        self.get_all_events()
    }

    /// # Save the event chain to a file
    ///
    /// Persists the entire event chain to a file on disk.
    ///
    /// ## Purpose
    ///
    /// This method allows saving the event chain for backup, analysis,
    /// or transferring to another system.
    ///
    /// ## Parameters
    ///
    /// * `path` - The file path where the chain should be saved
    ///
    /// ## Returns
    ///
    /// - `Ok(())` if the chain was successfully saved
    /// - `Err(anyhow::Error)` if an error occurred during saving
    ///
    /// ## Example
    ///
    /// ```rust
    /// use std::path::Path;
    ///
    /// let save_path = Path::new("./actor_events.chain");
    /// match store.save_chain(&save_path) {
    ///     Ok(_) => println!("Chain saved successfully"),
    ///     Err(e) => println!("Failed to save chain: {}", e),
    /// }
    /// ```
    ///
    /// ## Security
    ///
    /// The saved chain file contains the complete history of actor operations
    /// and state changes. Ensure appropriate file permissions and consider
    /// encryption for sensitive data.
    ///
    /// ## Implementation Notes
    ///
    /// This method acquires a lock on the chain during the save operation.
    /// For very large chains, this operation could be time-consuming and
    /// block other operations that need access to the chain.
    pub fn save_chain(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let chain = self.chain.lock().unwrap();
        chain.save_to_file(path)?;
        Ok(())
    }
}
