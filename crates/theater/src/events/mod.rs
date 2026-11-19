use crate::pack_bridge::{GraphValue, Value};
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
#[derive(Debug, Clone, Serialize, Deserialize, GraphValue)]
#[serde(tag = "category")]
pub enum ChainEventPayload {
    /// Standardized host function call with full I/O (for replay)
    HostFunction(HostFunctionCall),

    /// WASM execution events (function calls, results, errors)
    Wasm(wasm::WasmEventData),

    /// Replay completion summary
    ReplaySummary(replay::ReplaySummary),

    /// A structural actor-lifecycle transition (spawned / paused / resumed /
    /// terminated). The typed vocabulary that fate-links and monitors key on.
    /// The derived `"Lifecycle"` case-name is what the fate/monitor `Pattern`s
    /// match on (see [`crate::subscription`]).
    Lifecycle(lifecycle::ActorLifecycleEvent),
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

impl From<lifecycle::ActorLifecycleEvent> for ChainEventPayload {
    fn from(event: lifecycle::ActorLifecycleEvent) -> Self {
        ChainEventPayload::Lifecycle(event)
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
            packr_core::abi::encode(&Value::from(self.data.clone())).unwrap_or_else(|_| vec![]);
        ChainEvent {
            parent_hash,
            hash: vec![],
            event_type: self.event_type.clone(),
            data: encoded_data,
        }
    }
}

/// Pack-encode a host function call as a chain event's data, from references.
///
/// The bytes are exactly those of
/// `encode(&Value::from(ChainEventPayload::HostFunction(call)))`, but the
/// input and output trees are encoded where they are, not cloned into a
/// `HostFunctionCall` first. A `Value` is 88 bytes, so a 10 MB `list<u8>`
/// (a file read) is an 880 MB tree, and each clone of it was another 880 MB
/// (wisp ticket 0157). The test below checks the bytes against the derived
/// encoding.
pub fn encode_host_function_call(
    interface: &str,
    function: &str,
    input: &Value,
    output: &Value,
) -> Result<Vec<u8>, packr_abi::AbiError> {
    use packr_abi::{Encoder, GraphCodec, Node, NodeKind};
    fn name(payload: &mut Vec<u8>, s: &str) {
        payload.extend_from_slice(&(s.len() as u32).to_le_bytes());
        payload.extend_from_slice(s.as_bytes());
    }
    let mut enc = Encoder::new();
    // children first, in field order, as Value::Record's encode_graph does
    let children = [
        Value::String(interface.to_string()).encode_graph(&mut enc)?,
        Value::String(function.to_string()).encode_graph(&mut enc)?,
        input.encode_graph(&mut enc)?,
        output.encode_graph(&mut enc)?,
    ];
    let mut rec = Vec::new();
    name(&mut rec, "HostFunctionCall");
    rec.extend_from_slice(&4u32.to_le_bytes());
    for field in ["interface", "function", "input", "output"] {
        name(&mut rec, field);
    }
    for c in children {
        rec.extend_from_slice(&c.to_le_bytes());
    }
    let rec = enc.push_node(Node {
        kind: NodeKind::Record,
        payload: rec,
    });
    let mut var = Vec::new();
    name(&mut var, "ChainEventPayload");
    name(&mut var, "HostFunction");
    var.extend_from_slice(&0u32.to_le_bytes()); // tag of HostFunction
    var.extend_from_slice(&1u32.to_le_bytes());
    var.extend_from_slice(&rec.to_le_bytes());
    let root = enc.push_node(Node {
        kind: NodeKind::Variant,
        payload: var,
    });
    Ok(enc.finish(root).to_bytes())
}

/// Decode chain event data from pack-encoded bytes.
pub fn decode_chain_event_payload(data: &[u8]) -> Option<ChainEventPayload> {
    let value = packr_core::abi::decode(data).ok()?;
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

#[cfg(test)]
mod encode_tests {
    use super::*;

    fn derived(interface: &str, function: &str, input: &Value, output: &Value) -> Vec<u8> {
        let call = HostFunctionCall::new(interface, function, input.clone(), output.clone());
        packr_core::abi::encode(&Value::from(ChainEventPayload::HostFunction(call))).unwrap()
    }

    #[test]
    fn host_call_bytes_match_the_derived_encoding() {
        let bytes = Value::List {
            elem_type: crate::pack_bridge::ValueType::U8,
            items: (0..=255u8).map(Value::U8).collect(),
        };
        let cases = [
            (
                Value::String("big.txt".into()),
                Value::Result {
                    ok_type: crate::pack_bridge::ValueType::List(Box::new(
                        crate::pack_bridge::ValueType::U8,
                    )),
                    err_type: crate::pack_bridge::ValueType::String,
                    value: Ok(Box::new(bytes.clone())),
                },
            ),
            (Value::Tuple(vec![]), Value::U64(42)),
            (
                Value::Tuple(vec![
                    Value::S32(-1),
                    bytes,
                    Value::Option {
                        inner_type: crate::pack_bridge::ValueType::String,
                        value: None,
                    },
                ]),
                Value::Bool(true),
            ),
        ];
        for (input, output) in &cases {
            let ours =
                encode_host_function_call("theater:simple/filesystem", "read-file", input, output)
                    .unwrap();
            assert_eq!(
                ours,
                derived("theater:simple/filesystem", "read-file", input, output)
            );
            let call = decode_host_function_call(&ours).expect("decodes");
            assert_eq!((&call.input, &call.output), (input, output));
        }
    }
}

pub mod lifecycle;
pub mod replay;
pub mod wasm;
