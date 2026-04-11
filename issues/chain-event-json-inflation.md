# Chain Event JSON Inflation

## Problem

Chain event data is JSON-serialized via `serde_json::to_vec`, which inflates
binary data (like pack-encoded params/results) by roughly 3-4x. When this
inflated data is then passed back through pack as a `List<U8>` parameter to
supervisor callbacks like `handle-child-event`, it can exceed pack's
`max_sequence_len` (1,000,000) limit — even though the original data passed
through pack just fine.

## How it happens

### 1. Handler calls `tcp.send(conn_id, html_bytes)` (144KB)

Pack encodes this as an Array node. ~144KB encoded. Works fine.

### 2. Runtime records the call in the event chain

```rust
// runtime.rs:1028-1035
actor_instance.actor_store.record_event(ChainEventData {
    event_type: "wasm".to_string(),
    data: WasmEventData::WasmResult {
        function_name: name.clone(),
        result: result.clone(), // (Option<Value>, Vec<u8>)
    }.into(),
});
```

### 3. `ChainEventData::to_chain_event` JSON-serializes the payload

```rust
// events/mod.rs:103
data: serde_json::to_vec(&self.data).unwrap_or_else(|_| vec![]),
```

A `Vec<u8>` in JSON becomes `[72,84,84,80,47,49,46,49,...]` — each byte is
1-3 digits plus a comma separator. 144KB of raw bytes becomes ~400-500KB of
JSON text.

### 4. Supervisor forwards the chain event to the parent

```rust
// theater-handler-supervisor/src/lib.rs:268-274
let params = Value::Tuple(vec![
    Value::String(event.event_type.clone()),
    Value::List {
        elem_type: ValueType::U8,
        items: event.data.iter().map(|b| Value::U8(*b)).collect(),
    },
]);
```

The JSON bytes (~400-500KB for a single event) get passed as a `List<U8>`
through pack to the parent's `handle-child-event` export. For events with
larger payloads, this exceeds the 1M sequence limit.

### The irony

The original data was already pack-encoded. It passed through pack fine at
144KB. Then it got JSON-serialized (3-4x inflation), and then pack-encoded
*again*. The double-encoding through JSON is what pushes it over the limit.

## Where the JSON serialization lives

- `events/mod.rs:103` — `serde_json::to_vec(&self.data)` in `to_chain_event()`
- `chain/mod.rs:435` — `serde_json::to_vec(&event)` in `add_typed_event()`
  (serializes the whole event again for content-addressed storage)

The `ChainEventPayload` enum and its inner types (`WasmEventData`, etc.) all
derive `Serialize`/`Deserialize` for this JSON path.

## Suggested approach

The `WasmCall` and `WasmResult` event data already contains pack-encoded bytes
in its `params: Vec<u8>` and `result: (Option<Value>, Vec<u8>)` fields. These
bytes are a complete, self-describing encoding. JSON-serializing them is
redundant and expensive.

Options, from least to most invasive:

1. **Store pack-encoded bytes directly in chain events** — change
   `to_chain_event()` to use pack encoding instead of JSON for the data field.
   The params/results are already pack-encoded, so they can be embedded as-is.

2. **Use a compact JSON encoding for byte arrays** — instead of `[72,84,80,...]`,
   use base64 encoding for `Vec<u8>` fields via serde attributes
   (`#[serde(with = "base64")]`). This would reduce the inflation from 3-4x
   to ~1.33x.

3. **Don't embed raw params/results in chain events** — store them separately
   (e.g., in the content store by hash) and only reference them from the chain
   event. The chain event would contain a content hash instead of the full
   payload.

## Secondary issue: runtime kills actor on failed function calls

Even after fixing the encoding, the actor runtime (`runtime.rs:826-841`) sends
`ActorError` to the theater and pauses the actor on *any* failed
`CallFunctionPack` — including calls to optional supervisor callbacks. This
means a serialization error in an optional callback kills the parent actor.

The supervisor catches the error gracefully, but the runtime has already killed
the actor before the error even reaches the supervisor. The runtime should
return errors to callers without unconditionally treating them as fatal.
