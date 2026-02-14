# Runtime Simplification: Actor-Controlled Lifecycle

## Summary

Simplify the Theater runtime by:
1. Removing manifest as a runtime artifact (becomes build-time only)
2. Removing auto-start of handlers
3. Removing auto-call of `init()`
4. Making actors fully control their own lifecycle

Pack packages are self-describing via `__pack_types` metadata - the runtime doesn't need external configuration.

## Motivation

### Current Model (Runtime-Orchestrated)

```
manifest.toml → Runtime parses → Configures handlers → Starts handlers → Calls init() → Actor runs
```

**Problems:**
- Magic: behavior hidden in manifest, not visible in code
- Inflexible: handler configs fixed at deploy time
- Complex: runtime does a lot of orchestration
- Disconnected: feels different from normal programming

### Proposed Model (Actor-Controlled)

```
WASM bytes → Runtime satisfies imports → Caller invokes functions → Actor does what it wants
```

**Benefits:**
- Explicit: all behavior visible in actor code
- Flexible: actors configure things dynamically via function calls
- Simple: runtime just satisfies imports and routes calls
- Natural: feels like normal programming

## What Changes

### Manifest: Runtime Artifact → Build Tool

**Before (runtime config):**
```toml
name = "my-actor"
component = "target/my_actor.wasm"

[[handlers]]
name = "tcp"
listen = "0.0.0.0:8080"
max_connections = 50

[[handlers]]
name = "store"
```

**After (build config only, or eliminated entirely):**
```toml
[package]
name = "my-actor"

[build]
wit = ["wit/*.wit"]
target = "wasm32-unknown-unknown"
```

The manifest drives compilation. The resulting `.wasm` is self-describing and needs no manifest at runtime.

### Handler Configs: TOML → Function Arguments

**Before:**
```toml
[[handlers]]
name = "tcp"
listen = "0.0.0.0:8080"
max_connections = 50
```

**After (in actor code):**
```rust
let listener = tcp::listen("0.0.0.0:8080", TcpOptions {
    max_connections: 50,
})?;
```

Config becomes type-checked function arguments.

### Handler Lifecycle: Auto-Start → On-Demand

**Before:**
- Runtime reads manifest
- Instantiates handlers listed in manifest
- Calls `handler.start()` which spawns background tasks
- Handler runs independently of actor

**After:**
- Runtime loads WASM, queries `__pack_types` for imports
- Instantiates handlers that satisfy those imports
- Handlers provide host functions but don't auto-start background tasks
- Actor explicitly calls functions to initiate behavior

**Example - TCP handler:**

Before: Handler auto-listens on configured port, auto-accepts connections
After: Actor calls `tcp::listen()`, `tcp::accept()` when it wants

### Init Call: Automatic → Caller's Choice

**Before:**
```rust
// Runtime automatically calls after setup
fn init(state: Option<Vec<u8>>) -> Result<Option<Vec<u8>>, String> {
    // Must exist, always called
}
```

**After:**
```rust
// Caller decides what to call
let actor = runtime.spawn(wasm_bytes)?;
actor.call("my:interface.setup", config)?;  // Or don't call anything
actor.call("my:interface.process", data)?;
```

Actors don't need a special `init` function. Callers invoke whatever makes sense.

## The New Flow

### Spawning an Actor (from host)

```rust
let wasm_bytes = std::fs::read("my_actor.wasm")?;
let actor_id = theater.spawn(&wasm_bytes).await?;

// Query what it exports
let exports = theater.get_exports(actor_id).await?;

// Call whatever function you want
let result = theater.call(actor_id, "my:interface.start", args).await?;
```

### Spawning an Actor (from another actor)

```rust
// Using RPC handler
let child_bytes = store::get("child_actor.wasm")?;
let child = rpc::spawn(child_bytes)?;

// Configure it
child.call("configure", my_config)?;

// Use it
let result = child.call("process", data)?;
```

### Handler Instantiation

When WASM is loaded:
1. Runtime queries `__pack_types` to get list of imports
2. For each imported interface, find a registered handler that provides it
3. Verify interface hash compatibility
4. Set up host functions
5. WASM is ready (but handlers haven't "started" any background tasks)

When actor calls a function like `tcp::listen()`:
- The host function executes
- May spawn background tasks as needed
- Actor is in control

## What Stays the Same

- Pack packages and `__pack_types` metadata
- Interface hash verification
- Handler trait (but `start()` becomes optional/explicit)
- Host function registration via `setup_host_functions_composite()`
- Event chain recording

## Implementation Steps

1. [ ] Implement RPC handler (see `rpc-handler.md`) - needed for actor-to-actor spawning
2. [ ] Make `init()` call optional in ActorRuntime
3. [ ] Refactor handlers to not auto-start background tasks
4. [ ] Move handler configs to function arguments (update WIT interfaces)
5. [ ] Update spawn APIs to take WASM bytes directly
6. [ ] Remove manifest parsing from runtime (keep for CLI/build tools)
7. [ ] Update documentation and examples

## Migration Path

1. **Phase 1**: Add RPC handler, keep existing model working
2. **Phase 2**: Add `spawn(wasm_bytes)` API alongside manifest-based spawning
3. **Phase 3**: Deprecate auto-init, auto-start
4. **Phase 4**: Remove manifest-based spawning from runtime

Existing actors continue to work during migration.

## Open Questions

1. **Permissions/Security**: Without manifest, where do security policies come from?
   - Option A: Caller specifies when spawning
   - Option B: Encoded in WASM custom sections
   - Option C: Separate policy file (but that's manifest-like)

2. **Handler Discovery**: How does runtime know which handlers exist?
   - Currently: registered in TheaterRuntime at startup
   - Could stay the same - handlers are registered, WASM imports match to them

3. **Debugging/Observability**: Manifest provided useful metadata (name, etc.)
   - Could come from `__pack_types` metadata
   - Or caller provides when spawning

4. **CLI Experience**: `theater start manifest.toml` is nice UX
   - Could become `theater run actor.wasm`
   - Or manifest stays as CLI convenience, just not runtime requirement
