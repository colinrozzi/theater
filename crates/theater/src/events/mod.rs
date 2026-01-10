use crate::chain::ChainEvent;
use crate::replay::HostFunctionCall;
use serde::{Deserialize, Serialize};

/// # Chain Event Payload
///
/// The concrete event type for all events recorded in the actor's chain.
/// This replaces the previous generic event system with a simple, standardized format.
///
/// ## Event Categories
///
/// - **HostFunction**: All host function calls (interface, function, input, output)
/// - **Wasm**: WebAssembly execution events (calls, results, errors)
/// - **ReplaySummary**: Summary event recorded at the end of a replay
///
/// ## Design Philosophy
///
/// For deterministic replay, we only need to record:
/// 1. What WASM functions were called and their results
/// 2. What host functions were called and their I/O
///
/// This is sufficient to replay any actor execution with cryptographically
/// verifiable results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "category")]
pub enum ChainEventPayload {
    /// Standardized host function call with full I/O (for replay)
    HostFunction(HostFunctionCall),

    /// WASM execution events (function calls, results, errors)
    Wasm(wasm::WasmEventData),

    /// Replay completion summary
    ReplaySummary(replay::ReplaySummary),
}

impl From<HostFunctionCall> for ChainEventPayload {
    fn from(event: HostFunctionCall) -> Self {
        ChainEventPayload::HostFunction(event)
    }
}

impl From<wasm::WasmEventData> for ChainEventPayload {
    fn from(event: wasm::WasmEventData) -> Self {
        ChainEventPayload::Wasm(event)
    }
}

impl From<replay::ReplaySummary> for ChainEventPayload {
    fn from(event: replay::ReplaySummary) -> Self {
        ChainEventPayload::ReplaySummary(event)
    }
}

/// # Chain Event Data
///
/// `ChainEventData` is the structure for all typed events in the Theater system.
/// It wraps specific event data with an event type identifier.
///
/// ## Purpose
///
/// This struct serves as the bridge between strongly-typed event data and the
/// chain event system. It provides a standardized way to attach metadata to events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEventData {
    /// The type identifier for this event, used for filtering and routing.
    pub event_type: String,
    /// The specific event data payload.
    pub data: ChainEventPayload,
}

impl ChainEventData {
    /// Gets the event type identifier string.
    #[allow(dead_code)]
    pub fn event_type(&self) -> String {
        self.event_type.clone()
    }

    /// Serializes the event data to JSON.
    #[allow(dead_code)]
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Converts the typed event data to a chain event.
    ///
    /// ## Parameters
    ///
    /// * `parent_hash` - Optional hash of the parent event in the chain
    ///
    /// ## Returns
    ///
    /// A new `ChainEvent` with the serialized event data and metadata.
    /// The hash field will be empty - it's filled in by `StateChain::add_event`.
    pub fn to_chain_event(&self, parent_hash: Option<Vec<u8>>) -> ChainEvent {
        ChainEvent {
            parent_hash,
            hash: vec![],
            event_type: self.event_type.clone(),
            data: serde_json::to_vec(&self.data).unwrap_or_else(|_| vec![]),
        }
    }
}

pub mod replay;
pub mod runtime;
pub mod theater_runtime;
pub mod wasm;
