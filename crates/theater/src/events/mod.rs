use crate::pack_bridge::{ConversionError, IntoValue, Value};
use crate::replay::HostFunctionCall;
use serde::{Deserialize, Serialize};

// Import ChainEvent - note: this creates a circular dependency that we handle
// by having ChainEvent import ChainEventPayload from this module
use crate::chain::ChainEvent;

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

impl IntoValue for ChainEventPayload {
    fn into_value(self) -> Value {
        match self {
            ChainEventPayload::HostFunction(call) => Value::Variant {
                type_name: String::from("chain-event-payload"),
                case_name: String::from("host-function"),
                tag: 0,
                payload: vec![call.into_value()],
            },
            ChainEventPayload::Wasm(data) => Value::Variant {
                type_name: String::from("chain-event-payload"),
                case_name: String::from("wasm"),
                tag: 1,
                payload: vec![data.into_value()],
            },
            ChainEventPayload::ReplaySummary(summary) => Value::Variant {
                type_name: String::from("chain-event-payload"),
                case_name: String::from("replay-summary"),
                tag: 2,
                payload: vec![summary.into_value()],
            },
        }
    }
}

impl TryFrom<Value> for ChainEventPayload {
    type Error = ConversionError;

    fn try_from(v: Value) -> Result<Self, Self::Error> {
        match v {
            Value::Variant {
                case_name, payload, ..
            } => {
                let inner = payload
                    .into_iter()
                    .next()
                    .ok_or_else(|| ConversionError::MissingField("payload".into()))?;
                match case_name.as_str() {
                    "host-function" => Ok(ChainEventPayload::HostFunction(
                        HostFunctionCall::try_from(inner)?,
                    )),
                    "wasm" => Ok(ChainEventPayload::Wasm(wasm::WasmEventData::try_from(
                        inner,
                    )?)),
                    "replay-summary" => Ok(ChainEventPayload::ReplaySummary(
                        replay::ReplaySummary::try_from(inner)?,
                    )),
                    other => Err(ConversionError::ExpectedVariant(format!(
                        "unknown chain-event-payload case: {other}"
                    ))),
                }
            }
            other => Err(ConversionError::ExpectedVariant(format!("{:?}", other))),
        }
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
    /// A new `ChainEvent` with the pack-encoded event data and metadata.
    /// The hash field will be empty - it's filled in by `StateChain::add_event`.
    pub fn to_chain_event(&self, parent_hash: Option<Vec<u8>>) -> ChainEvent {
        let encoded_data =
            pack::abi::encode(&self.data.clone().into_value()).unwrap_or_else(|_| vec![]);
        ChainEvent {
            parent_hash,
            hash: vec![],
            event_type: self.event_type.clone(),
            data: encoded_data,
        }
    }
}

/// Decode chain event data from pack-encoded bytes.
pub fn decode_chain_event_payload(data: &[u8]) -> Option<ChainEventPayload> {
    let value = pack::abi::decode(data).ok()?;
    ChainEventPayload::try_from(value).ok()
}

/// Decode a HostFunctionCall from pack-encoded bytes.
pub fn decode_host_function_call(data: &[u8]) -> Option<HostFunctionCall> {
    let payload = decode_chain_event_payload(data)?;
    match payload {
        ChainEventPayload::HostFunction(call) => Some(call),
        _ => None,
    }
}

pub mod replay;
pub mod runtime;
pub mod theater_runtime;
pub mod wasm;
