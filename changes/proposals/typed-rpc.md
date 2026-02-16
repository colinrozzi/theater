# Typed RPC: Compile-Time Safe Actor-to-Actor Calls

## Summary

Replace the type-erased `rpc::call(actor_id, function, value)` pattern with fully typed imports that are bound to remote actors at runtime. This gives compile-time type safety while maintaining runtime flexibility.

## Motivation

The current RPC handler (see `rpc-handler.md`) uses a dynamic `Value` type:

```rust
// Current: type-erased, runtime checked
let result = rpc::call(calc_id, "my:calculator.add", Value::Tuple(vec![Value::S32(10), Value::S32(5)]), None)?;
// Hope it returns what we expect...
```

Problems:
1. **No compile-time type checking** - errors discovered at runtime
2. **Verbose** - manual Value construction/destruction
3. **Throws away type info** - Pack already computes interface hashes proving compatibility
4. **Easy to get wrong** - typos in function names, wrong param order

We want:
```rust
// Proposed: fully typed, compile-time checked
let sum = calculator::add(10, 5)?;  // Compiler verifies types!
```

## Design

### Core Insight

The caller imports an interface (typed, compile-time checked). The RPC handler *provides* that import by routing calls to a remote actor. Binding happens at runtime, but types are checked at compile time.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Compile Time                                                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  caller.wit:                    calculator.wit:                  │
│  ┌─────────────────────┐        ┌─────────────────────┐         │
│  │ world caller {      │        │ interface calculator│         │
│  │   import calculator │───────▶│   add: func(s32,s32)│         │
│  │   import rpc        │        │         -> s32      │         │
│  │ }                   │        └─────────────────────┘         │
│  └─────────────────────┘                                        │
│           │                                                      │
│           │ Compiler checks all calls to calculator::add         │
│           ▼                                                      │
│  caller.rs:                                                      │
│  ┌─────────────────────────────────────┐                        │
│  │ fn do_math() {                      │                        │
│  │   let sum = calculator::add(10, 5); │ ◀── Type checked!      │
│  │ }                                   │                        │
│  └─────────────────────────────────────┘                        │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ Link Time                                                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  RPC Handler sees:                                               │
│  - Caller imports "my/calculator"                                │
│  - Extracts interface definition from __pack_types               │
│  - Generates host functions matching the signature               │
│  - Initially: functions return "not bound" error                 │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ Runtime                                                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. Caller calls rpc::bind("my/calculator", actor_id)            │
│     - Handler looks up target actor                              │
│     - Verifies target exports "my/calculator"                    │
│     - Compares interface hashes (structural compatibility)       │
│     - If match: stores binding                                   │
│     - If mismatch: returns error                                 │
│                                                                  │
│  2. Caller calls calculator::add(10, 5)                          │
│     - Handler looks up binding for "my/calculator"               │
│     - Serializes params (format known from interface)            │
│     - Routes call to bound actor                                 │
│     - Deserializes result                                        │
│     - Returns to caller                                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### WIT Interface

```wit
// theater-rpc.wit

interface rpc {
    /// Bind an imported interface to a remote actor.
    ///
    /// This verifies that the target actor exports the interface
    /// with a compatible hash (structural type match).
    ///
    /// Example: rpc::bind("my/calculator", actor_id)
    bind: func(interface-name: string, actor-id: string) -> result<_, string>;

    /// Unbind an interface (calls will error until re-bound)
    unbind: func(interface-name: string) -> result<_, string>;

    /// Check if an interface is currently bound
    is-bound: func(interface-name: string) -> bool;

    /// Get the actor ID an interface is bound to (if any)
    bound-to: func(interface-name: string) -> option<string>;
}
```

### Multiple Targets

To call the same interface on multiple actors, use named imports:

```wit
world caller {
    import calc-primary: my/calculator;
    import calc-backup: my/calculator;
    import rpc: theater/rpc;
}
```

```rust
fn init() {
    rpc::bind("calc-primary", primary_actor_id)?;
    rpc::bind("calc-backup", backup_actor_id)?;
}

fn do_work() {
    // Both fully typed!
    let a = calc_primary::add(10, 5)?;
    let b = calc_backup::add(20, 3)?;
}
```

### Handler Implementation

```rust
pub struct TypedRpcHandler {
    theater_tx: Sender<TheaterCommand>,

    // Interface name -> binding info
    bindings: Arc<Mutex<HashMap<String, RpcBinding>>>,

    // Interface name -> expected hash (from caller's __pack_types)
    expected_hashes: HashMap<String, TypeHash>,

    // Interface name -> function signatures (for ser/de)
    interface_defs: HashMap<String, InterfaceDef>,
}

struct RpcBinding {
    actor_id: TheaterId,
    actor_handle: ActorHandle,
    interface_name: String,
}

impl Handler for TypedRpcHandler {
    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        // 1. Setup the rpc interface (bind/unbind/etc)
        self.setup_rpc_interface(builder)?;

        // 2. For each interface the actor imports that we should provide:
        //    - Extract interface def from __pack_types
        //    - Generate host functions that route to bound actor
        for (interface_name, interface_def) in &self.interface_defs {
            self.setup_remote_interface(builder, interface_name, interface_def)?;
        }

        Ok(())
    }
}

impl TypedRpcHandler {
    fn setup_remote_interface(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        interface_name: &str,
        interface_def: &InterfaceDef,
    ) -> Result<(), LinkerError> {
        let bindings = self.bindings.clone();
        let iface_name = interface_name.to_string();

        let mut iface_builder = builder.interface(interface_name)?;

        for func in &interface_def.functions {
            let func_name = func.name.clone();
            let full_name = format!("{}.{}", iface_name, func_name);
            let bindings = bindings.clone();

            iface_builder = iface_builder.func_async_result(
                &func_name,
                move |_ctx: AsyncCtx<ActorStore>, params: Value| {
                    let bindings = bindings.clone();
                    let full_name = full_name.clone();
                    let iface_name = iface_name.clone();

                    async move {
                        // Look up binding
                        let binding = {
                            let bindings = bindings.lock().unwrap();
                            bindings.get(&iface_name).cloned()
                        };

                        let Some(binding) = binding else {
                            return Err(Value::String(format!(
                                "Interface '{}' not bound. Call rpc::bind first.",
                                iface_name
                            )));
                        };

                        // Route call to bound actor
                        binding.actor_handle
                            .call_function(full_name, params)
                            .await
                            .map_err(|e| Value::String(e.to_string()))
                    }
                },
            )?;
        }

        Ok(())
    }
}
```

### Hash Verification at Bind Time

```rust
async fn bind(
    &self,
    interface_name: &str,
    actor_id: &str,
) -> Result<(), String> {
    // 1. Parse actor ID
    let actor_id = TheaterId::parse(actor_id)?;

    // 2. Get handle to target actor
    let handle = self.get_actor_handle(&actor_id).await?;

    // 3. Get target's interface hash
    let target_hash = handle.get_interface_hash(interface_name).await?;

    // 4. Compare with expected hash
    let expected_hash = self.expected_hashes.get(interface_name)
        .ok_or_else(|| format!("Unknown interface: {}", interface_name))?;

    if target_hash != *expected_hash {
        return Err(format!(
            "Interface hash mismatch for '{}'. \
             Caller expects {:?}, target has {:?}. \
             The interfaces are not structurally compatible.",
            interface_name, expected_hash, target_hash
        ));
    }

    // 5. Store binding
    let mut bindings = self.bindings.lock().unwrap();
    bindings.insert(interface_name.to_string(), RpcBinding {
        actor_id,
        actor_handle: handle,
        interface_name: interface_name.to_string(),
    });

    Ok(())
}
```

## Usage Examples

### Basic Usage

```rust
// calculator_actor.rs - exports the interface
#[export]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

// caller_actor.rs - imports and uses the interface
use calculator;  // Typed import!

fn init(calc_actor_id: String) -> Result<(), String> {
    // Bind at startup
    rpc::bind("my/calculator", &calc_actor_id)?;
    Ok(())
}

fn compute() -> i32 {
    // Fully typed call - compiler checks params and return type
    calculator::add(10, 5)
}
```

### Dynamic Discovery

```rust
fn init() -> Result<(), String> {
    // Get calculator actor ID from somewhere (config, spawn, lookup)
    let calc_id = supervisor::spawn("calculator.wasm")?;

    // Bind - this verifies compatibility
    rpc::bind("my/calculator", &calc_id)?;

    Ok(())
}
```

### Error Handling

```rust
fn init(calc_id: &str) -> Result<(), String> {
    // Bind can fail if:
    // - Actor doesn't exist
    // - Actor doesn't export the interface
    // - Interface hashes don't match
    match rpc::bind("my/calculator", calc_id) {
        Ok(()) => log("Bound to calculator"),
        Err(e) => {
            log(&format!("Failed to bind: {}", e));
            return Err(e);
        }
    }
    Ok(())
}

fn compute() -> Result<i32, String> {
    // Call can fail if:
    // - Not bound (forgot to call bind)
    // - Target actor crashed
    // - Function returned an error
    calculator::add(10, 5)
}
```

### Rebinding

```rust
fn switch_calculator(new_calc_id: &str) -> Result<(), String> {
    // Unbind old
    rpc::unbind("my/calculator")?;

    // Bind new
    rpc::bind("my/calculator", new_calc_id)?;

    Ok(())
}
```

## Requirements for Pack

The RPC handler needs Pack to provide:

1. **Interface extraction from `__pack_types`**
   - Given a WASM module, extract the interfaces it imports
   - For each interface, get the function signatures

2. **Hash computation/comparison**
   - Get the structural hash for an interface
   - Already implemented for handler matching

3. **Host function generation**
   - Given an interface definition, generate matching host functions
   - Functions use Pack's serialization format

## Implementation Steps

1. [ ] **Extend Pack**: Add API to extract imported interfaces from `__pack_types`
2. [ ] **Extend Pack**: Add API to get function signatures for an interface
3. [ ] **Handler Discovery**: Detect which imports the RPC handler should provide
4. [ ] **Handler Setup**: Generate host functions for each remote interface
5. [ ] **Bind Implementation**: Hash verification and binding storage
6. [ ] **Call Routing**: Route calls through bindings to target actors
7. [ ] **Error Handling**: Clean errors for unbound, mismatch, etc.
8. [ ] **Testing**: Integration tests with calculator example

## Open Questions

1. **Which imports does RPC handle?**
   - Option A: Explicit declaration in manifest
   - Option B: Convention (e.g., interfaces not satisfied by other handlers)
   - Option C: All imports, RPC is the fallback provider

   Recommendation: Option C - RPC provides any import not satisfied by another handler

2. **What if binding fails mid-operation?**
   - Actor crashes, handle becomes invalid
   - Currently: next call fails
   - Could add: automatic rebind, health checks

3. **Async considerations?**
   - Current design is synchronous call/response
   - Future: streaming responses, bidirectional channels
   - Keep simple for now, extend later

4. **Cycles?**
   - Actor A calls B, B calls A (same call stack)
   - Risk of deadlock with synchronous calls
   - Document as known limitation
   - Future: async/channel patterns for bidirectional

## Relationship to Existing RPC Handler

This proposal supersedes `rpc-handler.md`. The existing implementation uses:
- `rpc::call(actor_id, function, value)` - type-erased

This proposal changes to:
- Typed imports + `rpc::bind(interface, actor_id)`

The `call` function could remain as an escape hatch for truly dynamic cases, but typed imports should be the primary pattern.

## Benefits

1. **Compile-time type safety** - catch errors early
2. **Clean syntax** - `calc::add(10, 5)` not `rpc::call(..., Value::Tuple(...))`
3. **IDE support** - autocomplete, go-to-definition work
4. **Leverages Pack** - uses existing hash verification
5. **Flexible binding** - static types, dynamic targets
