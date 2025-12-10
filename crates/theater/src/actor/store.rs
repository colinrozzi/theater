//! # Actor Store
//!
//! This module provides an abstraction for sharing resources between an actor and the Theater runtime system.
//! The ActorStore serves as a container for state, event chains, and communication channels that are
//! needed for WASM host functions to interface with the Actor system.

use crate::actor::handle::ActorHandle;
use crate::chain::{ChainEvent, StateChain};
use crate::events::{ChainEventData, EventData, EventPayload};
use crate::id::TheaterId;
use crate::messages::{MessageCommand, TheaterCommand};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc::Sender;

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
#[derive(Clone)]
pub struct ActorStore<E = EventData>
where
    E: EventPayload,
{
    /// Unique identifier for the actor
    pub id: TheaterId,

    /// Channel for sending commands to the Theater runtime
    pub theater_tx: Sender<TheaterCommand>,

    /// Optional channel for sending message commands to the message-server handler
    /// This is only available when the message-server handler is loaded
    pub message_tx: Option<Sender<MessageCommand>>,

    /// The event chain that records all actor operations for verification and audit
    pub chain: Arc<RwLock<StateChain<E>>>,

    /// The current state of the actor, stored as a binary blob
    pub state: Option<Vec<u8>>,

    /// Handle to interact with the actor
    pub actor_handle: ActorHandle,
}

impl<E> ActorStore<E>
where
    E: EventPayload,
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
        message_tx: Option<Sender<MessageCommand>>,
        actor_handle: ActorHandle,
        chain: Arc<RwLock<StateChain<E>>>,
    ) -> Self {
        Self {
            id: id.clone(),
            theater_tx: theater_tx.clone(),
            message_tx,
            chain,
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

    // NEW METHODS: These benefit significantly from RwLock's concurrent read access

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
