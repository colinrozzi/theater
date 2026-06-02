//! # Event Chain System
//!
//! The `chain` module defines Theater's content-addressed event type and the
//! per-actor head-hash that keeps successive events cryptographically linked.
//!
//! Events are **not retained** by the runtime: each `ChainEvent` is hashed,
//! broadcast to subscribers, used to update the actor's head hash, and dropped.
//! Anything that wants a durable record (replay, audit, debug tail) must
//! subscribe via [`StateChain::subscribe`] and persist on its own.
//!
//! ## Core Features
//!
//! * **Cryptographic linking**: events carry `parent_hash` referring to the
//!   previous event's `hash`, so a subscriber can verify the chain as it
//!   streams.
//! * **Content-addressed**: each event's hash is `H(serialize(parent_hash,
//!   event_type, data))`.
//! * **Tail-only broadcast**: subscribers see events emitted from the moment
//!   they subscribe; there is no backfill of historical events.

use std::fmt;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tracing::{debug, warn};
use wasmtime::component::{ComponentType, Lift, Lower};

use crate::events::ChainEventData;
use crate::messages::TheaterCommand;
use crate::store::ContentRef;
use crate::TheaterId;
use theater_chain::event::EventType;

/// Wrapper type for replay chain events stored in ActorStore extensions.
/// Used by handlers like WasiHttpHandler to detect replay mode and access recorded events.
#[derive(Debug, Clone)]
pub struct HttpReplayChain(pub Vec<ChainEvent>);

impl HttpReplayChain {
    /// Get events filtered by event type.
    pub fn events_by_type(&self, event_type: &str) -> Vec<&ChainEvent> {
        self.0
            .iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }

    /// Get all HTTP incoming handler events.
    pub fn http_incoming_events(&self) -> Vec<&ChainEvent> {
        self.events_by_type("wasi:http/incoming-handler@0.2.0/handle")
    }
}

/// A single immutable event in an actor's execution history.
///
/// Each event carries the hash of its parent, forming a cryptographically
/// linked chain. The runtime computes the hash and broadcasts the event;
/// retention is the subscriber's responsibility.
#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower, Eq)]
#[component(record)]
pub struct ChainEvent {
    /// Content hash of this event (over `parent_hash`, `event_type`, `data`).
    pub hash: Vec<u8>,
    /// Hash of the parent event, or `None` for the first event in the chain.
    #[component(name = "parent-hash")]
    pub parent_hash: Option<Vec<u8>>,
    /// Type identifier for the event (e.g. `"state_change"`, `"http_request"`).
    #[component(name = "event-type")]
    pub event_type: String,
    /// Event payload, serialized.
    pub data: Vec<u8>,
}

impl fmt::Display for ChainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hash_str = hex::encode(&self.hash);
        let short_hash = if hash_str.len() > 7 {
            &hash_str[0..7]
        } else {
            &hash_str
        };

        let parent_str = match &self.parent_hash {
            Some(ph) => {
                let ph_str = hex::encode(ph);
                if ph_str.len() > 7 {
                    format!("(parent: {}...)", &ph_str[0..7])
                } else {
                    format!("(parent: {})", ph_str)
                }
            }
            None => "(root)".to_string(),
        };

        let content = if let Ok(value) = packr::abi::decode(&self.data) {
            format!("{}", value)
        } else if let Ok(text) = std::str::from_utf8(&self.data) {
            let preview = if text.len() > 80 {
                format!("{}...", &text[0..77])
            } else {
                text.to_string()
            };
            format!("'{}'", preview)
        } else {
            format!("{} bytes", self.data.len())
        };

        write!(
            f,
            "Event[{}] {} {} {}",
            short_hash,
            parent_str,
            console::style(&self.event_type).cyan(),
            content
        )
    }
}

impl EventType for ChainEvent {
    fn event_type(&self) -> String {
        self.event_type.clone()
    }

    fn len(&self) -> usize {
        self.data.len()
    }
}

impl PartialEq for ChainEvent {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl std::hash::Hash for ChainEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

/// Per-actor chain state.
///
/// Holds only the **head hash** (the hash of the most recently emitted event)
/// plus the broadcast channel that subscribers tap. Events themselves are not
/// retained; they are constructed, hashed, broadcast, and dropped.
///
/// ## Subscriber semantics
///
/// `subscribe()` returns a `broadcast::Receiver` that sees events from the
/// moment of subscription forward. There is no backfill. A subscriber that
/// wants the full history must subscribe before the actor begins emitting and
/// retain the events itself.
#[derive(Clone)]
pub struct StateChain {
    /// Hash of the most recently emitted event, or `None` if no event has been
    /// emitted yet. Used as `parent_hash` for the next event.
    current_hash: Option<Vec<u8>>,
    /// Channel for notifying the Theater runtime of new events.
    theater_tx: Sender<TheaterCommand>,
    /// Identifier of the actor that owns this chain.
    actor_id: TheaterId,
    /// Broadcast channel for direct event subscription.
    event_broadcast: broadcast::Sender<ChainEvent>,
}

impl fmt::Debug for StateChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateChain")
            .field("current_hash", &self.current_hash)
            .field("actor_id", &self.actor_id)
            .finish()
    }
}

impl StateChain {
    /// Creates a new empty state chain for an actor.
    pub fn new(actor_id: TheaterId, theater_tx: Sender<TheaterCommand>) -> Self {
        let (event_broadcast, _) = broadcast::channel(1024);

        Self {
            current_hash: None,
            theater_tx,
            actor_id,
            event_broadcast,
        }
    }

    /// Adds a new typed event to the chain.
    ///
    /// Computes the event's hash from the current head, broadcasts it to
    /// subscribers, advances the head, and drops the event. The runtime is
    /// notified via `theater_tx` for cross-actor visibility.
    pub fn add_typed_event(
        &mut self,
        event_data: ChainEventData,
    ) -> Result<ChainEvent, serde_json::Error> {
        let mut event = event_data.to_chain_event(self.current_hash.clone());

        let serialized_event = serde_json::to_vec(&event)?;
        let content_ref = ContentRef::from_content(&serialized_event);
        let hash_bytes = hex::decode(content_ref.hash()).unwrap();
        event.hash = hash_bytes.clone();

        self.current_hash = Some(event.hash.clone());

        if let Err(e) = self.theater_tx.try_send(TheaterCommand::NewEvent {
            actor_id: self.actor_id,
            event: event.clone(),
        }) {
            warn!("Failed to send event notification: {}", e);
        }

        // Broadcast to direct subscribers. Send errors mean no active
        // subscribers — that's fine, the event is dropped.
        let _ = self.event_broadcast.send(event.clone());

        debug!(
            "Emitted event {} for actor {}",
            content_ref.hash(),
            self.actor_id
        );

        Ok(event)
    }

    /// Returns the hash of the most recently emitted event.
    pub fn head_hash(&self) -> Option<&[u8]> {
        self.current_hash.as_deref()
    }

    /// Subscribe to events as they are emitted.
    ///
    /// Returns a broadcast receiver that sees each event from the moment of
    /// subscription forward. There is no backfill of prior events.
    pub fn subscribe(&self) -> broadcast::Receiver<ChainEvent> {
        self.event_broadcast.subscribe()
    }
}
