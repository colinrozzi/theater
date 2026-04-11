# Resolved: Chain Event JSON Inflation

Resolves `chain-event-json-inflation.md`.

## What changed

Chain event payloads are now pack-encoded instead of JSON-serialized. Combined
with pack-abi's new `Array` node kind (compact primitive list encoding), this
eliminates the 3-4x byte inflation that was causing supervisor child-event
delivery to exceed pack's sequence limits.

### Before

```
actor sends 144KB response
  -> runtime records chain event
  -> serde_json::to_vec serializes Vec<u8> as [72,84,84,80,...]  (~400-500KB)
  -> supervisor forwards to parent as List<U8>
  -> pack encodes 400-500K individual nodes -> exceeds limits
```

### After

```
actor sends 144KB response
  -> runtime records chain event
  -> pack::abi::encode serializes payload with Array node  (~144KB)
  -> supervisor forwards to parent as List<U8>
  -> pack encodes as compact Array node -> ~144KB total
```

The double-encoding through JSON is gone. Pack-encoded data stays pack-encoded
all the way through the chain event pipeline.

## How it works

### Pack-native serialization for chain events

`ChainEventPayload` and its inner types (`HostFunctionCall`, `WasmEventData`,
`ReplaySummary`) now implement `IntoValue` and `TryFrom<Value>`, mapping to
pack's type system:

```
ChainEventPayload -> Variant {
    "host-function" -> Record { interface, function, input, output }
    "wasm"          -> Variant {
        "wasm-call"   -> Record { function-name, params: list<u8> }
        "wasm-result" -> Record { function-name, value: option<Value>, bytes: list<u8> }
        "wasm-error"  -> Record { function-name, message }
        "wasm-component-creation-error" -> Record { error }
    }
    "replay-summary" -> Record { total-events, events-replayed, mismatches, success, error }
}
```

### Encoding path

`ChainEventData::to_chain_event()` now calls `pack::abi::encode()` instead of
`serde_json::to_vec()` to produce the `ChainEvent.data: Vec<u8>` field.

### Decoding path

Two helper functions in `events/mod.rs` handle decoding with backward
compatibility:

- `decode_chain_event_payload(data)` — tries pack decode first, falls back to
  JSON for old chain data
- `decode_host_function_call(data)` — same pattern, also tries unwrapping from
  a full `ChainEventPayload`

All decoder sites (interceptor, replay handler, CLI display) use these helpers.

### Pack-abi Array node kind

Pack-abi now has a compact `Array` node (`0x15`) for lists of fixed-size
primitive types. A `List<U8>` of N bytes encodes as a single node with N bytes
of contiguous payload, instead of N individual graph nodes. Theater picks this
up automatically via the path dependency — no code changes needed for the
compact encoding, the encoder routes `Value::List { elem_type: U8, ... }` to
`Array` transparently.

## Files changed

- `crates/theater/src/events/mod.rs` — pack encoding in `to_chain_event()`,
  decode helpers
- `crates/theater/src/events/wasm.rs` — `IntoValue`/`TryFrom<Value>` for
  `WasmEventData`
- `crates/theater/src/events/replay.rs` — `IntoValue`/`TryFrom<Value>` for
  `ReplaySummary`
- `crates/theater/src/replay/mod.rs` — `IntoValue`/`TryFrom<Value>` for
  `HostFunctionCall`
- `crates/theater/src/pack_bridge.rs` — `IntoValue for Value` identity impl
- `crates/theater/src/interceptor.rs` — uses decode helpers
- `crates/theater/src/replay/handler.rs` — uses decode helpers
- `crates/theater-cli/src/commands/start.rs` — graceful display for
  pack-encoded data

## What remains

The secondary issue from the original bug report — `handle-child-event` errors
being fatal when they should be non-fatal — is not addressed here. That is a
separate concern in the runtime error propagation path.
