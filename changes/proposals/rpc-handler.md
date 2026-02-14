# RPC Handler: Direct Actor-to-Actor Function Calls

## Summary

Add an RPC handler that enables actors to call functions on other actors directly with full type safety. This replaces the message-passing model (send JSON bytes, receive JSON bytes) with direct typed function calls.

## Motivation

Currently, actors communicate via the message-server handler:
```rust
// Actor A sends bytes to Actor B
message_server::send(actor_b_id, json_bytes)?;

// Actor B receives bytes, deserializes, processes
fn handle_send(msg: Vec<u8>) { ... }
```

This works but:
1. No compile-time type checking
2. Manual serialization/deserialization
3. Doesn't leverage Pack's typed interfaces
4. Feels like glue code rather than direct calls

With Pack's `__pack_types` metadata and interface hashing, we can do better:
```rust
// Actor A calls Actor B directly
let calc = rpc::spawn(calculator_wasm_bytes)?;
let result = calc.call("add", (1, 2))?;  // Typed!
```

## Design

### WIT Interface

```wit
interface rpc {
    /// Options for RPC calls
    record call-options {
        /// Timeout in milliseconds (None = default timeout)
        timeout-ms: option<u64>,
        // Future options: retry policy, tracing context, etc.
    }

    /// Call a function on an actor
    ///
    /// The function name uses "interface.function" format,
    /// e.g., "my:calculator.add"
    call: func(
        actor-id: string,
        function: string,
        params: value,
        options: option<call-options>
    ) -> result<value, string>;

    /// Check if actor exports an interface (uses hash comparison)
    implements: func(actor-id: string, interface: string) -> result<bool, string>;

    /// Get actor's exported interface names
    exports: func(actor-id: string) -> result<list<string>, string>;
}

/// Dynamic value type (recursive, wit+ enabled)
///
/// This allows passing arbitrary typed data between actors
/// without pre-declaring every possible type combination.
variant value {
    null,
    bool(bool),
    s8(s8), s16(s16), s32(s32), s64(s64),
    u8(u8), u16(u16), u32(u32), u64(u64),
    f32(f32), f64(f64),
    char(char),
    string(string),
    list(list<value>),
    record(list<tuple<string, value>>),
    variant(tuple<string, option<value>>),
    option(option<value>),
    result(result<value, value>),
}
```

Note: Actor lifecycle (spawn, stop) is handled by the supervisor handler, not RPC.
RPC is purely for calling functions on existing actors.

### Handler Implementation

```rust
pub struct RpcHandler {
    theater_tx: Sender<TheaterCommand>,
}

impl RpcHandler {
    fn rpc_interface() -> InterfaceImpl {
        InterfaceImpl::new("theater:simple/rpc")
            .func("call", |_: String, _: String, _: Value, _: Option<CallOptions>| -> Result<Value, String> {
                Ok(Value::Null)
            })
            .func("implements", |_: String, _: String| -> Result<bool, String> {
                Ok(false)
            })
            .func("exports", |_: String| -> Result<Vec<String>, String> {
                Ok(vec![])
            })
    }
}
```

The handler is stateless - no connections to track. Each `call` looks up the target actor via TheaterRuntime and forwards the request.

### Usage Example

```rust
// calculator.rs (Actor B)
#[export(wit = "my:calculator.add")]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

// main.rs (Actor A)
fn run() -> Result<(), String> {
    // Spawn calculator actor using supervisor
    let calc_id = supervisor::spawn("calculator.wasm", None)?;

    // Verify it implements what we need (optional, for safety)
    if !rpc::implements(calc_id, "my:calculator")? {
        return Err("Calculator doesn't implement expected interface".into());
    }

    // Call directly - simple case, no options
    let result = rpc::call(calc_id, "my:calculator.add", (1, 2), None)?;
    // result is Value::S32(3)

    // Call with timeout
    let result = rpc::call(calc_id, "my:calculator.multiply", (3, 4), Some(CallOptions {
        timeout_ms: Some(5000),
    }))?;

    Ok(())
}
```

### Interface Verification

The `rpc::implements` function enables callers to verify compatibility before calling:
1. Handler queries target actor's `__pack_types` metadata
2. Computes interface hash for the requested interface
3. Compares against known handler/interface hashes for O(1) compatibility check

This is optional - callers can skip verification and just call directly if they trust the target.

### Typed Call Wrapper (Future Enhancement)

With code generation, we could generate typed stubs:
```rust
// Generated from calculator.wit
mod calculator {
    pub fn add(handle: ActorHandle, a: i32, b: i32) -> Result<i32, String> {
        let result = rpc::call(handle, "my:calculator.add", (a, b))?;
        // Decode result...
    }
}

// Usage becomes:
let result = calculator::add(calc, 1, 2)?;
```

## Relationship to Runtime Simplification

This RPC handler is a key enabler for removing manifest-based handler configuration. Instead of declaring dependencies in TOML, actors explicitly spawn/connect to what they need in code.

See: `runtime-simplification.md`

## Implementation Steps

1. [ ] Define WIT interface for RPC handler
2. [ ] Implement `call` host function (core functionality)
3. [ ] Add `implements` for interface verification via hashes
4. [ ] Add `exports` for introspection
5. [ ] Add timeout support via call-options
6. [ ] Consider typed stub generation (future enhancement)

## Open Questions

1. **Error handling**: How to propagate errors from Actor B to Actor A?
   - Errors returned as `result<value, string>` - the string contains error info
   - Should we have structured error types?

2. **Cycles**: Can Actor A call Actor B which calls Actor A?
   - Likely deadlock risk if synchronous
   - May need async/channel-based patterns for bidirectional communication

3. **Backpressure**: How to handle slow actors?
   - Timeout in call-options handles basic case
   - More sophisticated backpressure could be future enhancement
