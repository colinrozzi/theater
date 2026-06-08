//! # Event Chain System
//!
//! The `chain` module defines Theater's content-addressed event type and the
//! per-actor head-hash that keeps successive events cryptographically linked.
//!
//! Events are **not retained** by the runtime: each `ChainEvent` is hashed,
//! broadcast to subscribers, used to update the actor's head hash, and dropped.
//! Anything that wants a durable record (replay, audit, debug tail) must
//! subscribe via [`StateChain::subscribe`] or [`StateChain::add_subscriber`]
//! and persist on its own.
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
//!
//! ## Subscription topology
//!
//! Subscriber dispatch is **direct from the chain**, not routed through the
//! runtime command channel. This decouples event flow from the runtime's
//! control plane: a lagging subscriber cannot stall `TheaterRuntime::run()`,
//! and `theater_tx` (the control channel that carries spawn/stop/etc.) is
//! never pressured by event traffic. This is the structural fix for the
//! 2026-06-05 sentinel cutover wedge — see project notes.

use std::fmt;

use tokio::sync::mpsc::Sender;
use tracing::{debug, error};

use crate::events::ChainEventData;
use crate::store::ContentRef;
use crate::TheaterId;

pub use theater_chain::ChainEvent;

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

/// Per-actor chain state.
///
/// Holds the head hash and a list of direct mpsc subscribers populated via
/// [`add_subscriber`]. Events are constructed, hashed, dispatched, and
/// dropped — no retention.
///
/// ## Subscriber semantics
///
/// `add_subscriber(tx)` registers a `tokio::sync::mpsc::Sender`. Each
/// receives `(actor_id, event)` tuples so a single subscriber can
/// multiplex from many actors. Emission dispatches via `.send().await`:
/// a lagging subscriber back-pressures the producer (the actor's host
/// call awaits subscriber capacity), so chain completeness is preserved.
/// The subscriber side controls overflow policy by how it drains its
/// receiver:
///
/// * **Strict** — read the mpsc in the main loop; producer back-pressures
///   on the channel's capacity. The producing actor's host calls block
///   until the subscriber catches up.
/// * **Best-effort** — spawn a drainer task that pulls into a local ring
///   buffer; the mpsc is drained at line rate, drops happen in the
///   subscriber's own buffer on its own terms — producer never blocks.
///
/// A slow subscriber CANNOT stall the runtime command loop — emission is
/// decoupled from `theater_tx`. It CAN stall the actor whose events it
/// receives (intentional back-pressure).
pub struct StateChain {
    /// Hash of the most recently emitted event, or `None` if no event has been
    /// emitted yet. Used as `parent_hash` for the next event.
    current_hash: Option<Vec<u8>>,
    /// Identifier of the actor that owns this chain. Used for diagnostic logs.
    actor_id: TheaterId,
    /// Direct mpsc subscribers registered via `add_subscriber`. Each
    /// receives `(actor_id, event)` tuples. Emission awaits each sender;
    /// closed senders are evicted on next emission.
    subscribers: Vec<Sender<(TheaterId, ChainEvent)>>,
}

impl fmt::Debug for StateChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateChain")
            .field("current_hash", &self.current_hash)
            .field("actor_id", &self.actor_id)
            .field("subscribers", &self.subscribers.len())
            .finish()
    }
}

impl StateChain {
    /// Creates a new empty state chain for an actor.
    pub fn new(actor_id: TheaterId) -> Self {
        Self {
            current_hash: None,
            actor_id,
            subscribers: Vec::new(),
        }
    }

    /// Adds a new typed event to the chain.
    ///
    /// Computes the event's hash from the current head, advances the head,
    /// then dispatches to subscribers via `.send().await`. A lagging
    /// subscriber back-pressures the caller — chain completeness is
    /// preserved. Closed senders are evicted.
    pub async fn add_typed_event(
        &mut self,
        event_data: ChainEventData,
    ) -> Result<ChainEvent, serde_json::Error> {
        let mut event = event_data.to_chain_event(self.current_hash.clone());

        let serialized_event = serde_json::to_vec(&event)?;
        let content_ref = ContentRef::from_content(&serialized_event);
        let hash_bytes = hex::decode(content_ref.hash()).unwrap();
        event.hash = hash_bytes.clone();

        self.current_hash = Some(event.hash.clone());

        // Dispatch to subscribers with back-pressure. Track closed senders
        // for eviction after the loop (can't mutate self.subscribers while
        // iterating over it).
        let actor_id = self.actor_id;
        let mut closed_indices: Vec<usize> = Vec::new();
        for (index, subscriber) in self.subscribers.iter().enumerate() {
            if subscriber.send((actor_id, event.clone())).await.is_err() {
                error!("Subscriber for actor {:?} closed; evicting", self.actor_id);
                closed_indices.push(index);
            }
        }
        for index in closed_indices.into_iter().rev() {
            self.subscribers.swap_remove(index);
        }

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

    /// Register a direct mpsc subscriber.
    ///
    /// The chain dispatches each new event to `tx` via `try_send` as
    /// `(actor_id, event)`. The subscriber must drain its receiver fast
    /// enough (or wrap it in a drainer task) to avoid dropped events
    /// under burst load. See type docs for the back-pressure / best-effort
    /// tradeoff.
    ///
    /// Termination is signaled by the actor's terminal chain event
    /// (`WasmError` for crashes, `"shutdown"` for normal exit), followed
    /// by the channel closing — no separate error path.
    pub fn add_subscriber(&mut self, tx: Sender<(TheaterId, ChainEvent)>) {
        self.subscribers.push(tx);
    }

    /// Remove a previously-registered subscriber, identified by channel
    /// identity (`tokio::sync::mpsc::Sender::same_channel`).
    ///
    /// The supervisor-side opt-in subscription model registers a clone of
    /// the parent supervisor handler's single aggregated event sender on
    /// each subscribed child's chain. `same_channel` matches all clones
    /// that route to the same receiver, so the parent can unsubscribe from
    /// a specific child by passing its own event sender.
    ///
    /// Idempotent — returns `true` if a subscriber was removed, `false`
    /// if no matching subscriber existed. Closed senders are also evicted
    /// passively on the next emission, so calling this for a child whose
    /// chain has already torn down is harmless.
    pub fn remove_subscriber(&mut self, tx: &Sender<(TheaterId, ChainEvent)>) -> bool {
        if let Some(index) = self.subscribers.iter().position(|s| s.same_channel(tx)) {
            self.subscribers.swap_remove(index);
            true
        } else {
            false
        }
    }
}
