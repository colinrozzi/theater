//! Diagnostic: does a `wasm` chain event survive the encode→decode round-trip
//! that lifecycle-monitor delivery depends on?
//!
//! The lifecycle handler's delivery loop does:
//!     let value = match decode_chain_event_payload(&event.data) {
//!         Some(payload) => Value::from(payload),
//!         None => continue,            // <-- silently drops the event
//!     };
//! so if a `WasmCall`/`WasmResult` payload (which carries a nested raw packr
//! `Value` in `params`/`response`) fails to decode, a monitor of that actor
//! sees NOTHING for every rpc.call / function invocation. This test reproduces
//! exactly that path.

use theater::events::wasm::WasmEventData;
use theater::events::{decode_chain_event_payload, ChainEventData, ChainEventPayload};
use theater::pack_bridge::Value;

fn roundtrip(data: ChainEventData) -> Option<ChainEventPayload> {
    let ev = data.to_chain_event(None);
    assert!(
        !ev.data.is_empty(),
        "encode produced empty bytes (encode failed)"
    );
    decode_chain_event_payload(&ev.data)
}

#[test]
fn wasm_call_event_roundtrips_for_monitor_delivery() {
    let decoded = roundtrip(ChainEventData {
        event_type: "wasm".to_string(),
        data: ChainEventPayload::Wasm(WasmEventData::WasmCall {
            function_name: "theater:simple/wisp.evaluate".to_string(),
            params: Value::Tuple(vec![Value::S64(1), Value::S64(2)]),
        }),
    });
    assert!(
        decoded.is_some(),
        "WasmCall failed round-trip -> lifecycle monitor drops it (None => continue)"
    );
}

#[test]
fn wasm_result_event_roundtrips_for_monitor_delivery() {
    let decoded = roundtrip(ChainEventData {
        event_type: "wasm".to_string(),
        data: ChainEventPayload::Wasm(WasmEventData::WasmResult {
            function_name: "theater:simple/wisp.evaluate".to_string(),
            response: Value::S64(3),
        }),
    });
    assert!(
        decoded.is_some(),
        "WasmResult failed round-trip -> lifecycle monitor drops it (None => continue)"
    );
}

#[test]
fn wasm_error_event_roundtrips_for_monitor_delivery() {
    // No nested Value here -- control case. If THIS passes but the above fail,
    // the nested raw `Value` field is the culprit.
    let decoded = roundtrip(ChainEventData {
        event_type: "wasm".to_string(),
        data: ChainEventPayload::Wasm(WasmEventData::WasmError {
            function_name: "f".to_string(),
            message: "boom".to_string(),
        }),
    });
    assert!(decoded.is_some(), "WasmError failed round-trip");
}
