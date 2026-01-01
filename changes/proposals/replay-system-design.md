# Replay System Design Proposal

## Overview

This proposal outlines the design for Theater's replay system - the ability to replay an actor's execution from its chain and verify determinism. The key insight is that WASM execution is deterministic, so given the same inputs, an actor should produce the same outputs.

## Goals

1. **Verify chains**: Given a component and a chain, verify the chain was produced by that component
2. **Handler-less replay**: Replay without needing the actual handler implementations
3. **Hash-based divergence detection**: Compare event hashes to detect where chains diverge
4. **No runtime changes**: Implement replay as a handler, not a separate runtime

## Core Concept

**The chain is all you need to replay an actor.**

The chain is fully self-describing - it contains everything needed to replay without any handler crates or external dependencies:

1. **WIT type definitions** → How to set up the linker (function signatures)
2. **Recorded I/O** → What to return from stub functions
3. **Event sequence** → What calls to replay and in what order

**Replay is just another handler.** The runtime provides infrastructure to run actors; handlers drive them and provide external interfaces. The ReplayHandler:
- **Provides**: Stub implementations of all imports (constructed from chain's WIT definitions)
- **Drives**: Replays function calls from chain events

```
Normal:   Actor → HTTP Handler → External System → Response → Chain
Replay:   Actor → ReplayHandler → Chain (recorded response) → New Chain
```

The new chain's hashes should match the original if the component is deterministic.

## Architecture

### ReplayHandler Design

```rust
pub struct ReplayHandler {
    /// The chain to replay
    chain: Vec<ChainEvent>,

    /// Interfaces used in the chain (extracted from event_type prefixes)
    interfaces_used: HashSet<String>,

    /// Cursor tracking which event to return next for each interface+function
    replay_cursors: HashMap<String, usize>,

    /// Results of the replay
    result: ReplayResult,
}

impl<E> Handler<E> for ReplayHandler {
    fn setup_host_functions(&mut self, component: &mut ActorComponent<E>, ctx: &mut HandlerContext) -> Result<()> {
        // For each interface used in the chain, register stub functions
        // that return the recorded responses
    }

    fn start(&mut self, actor_handle: ActorHandle, ...) -> ... {
        // Iterate through chain, find WasmCall events, replay them
        // Compare resulting event hashes with originals
    }

    fn imports(&self) -> Option<Vec<String>> {
        // Return all interfaces extracted from chain
        Some(self.interfaces_used.iter().cloned().collect())
    }

    fn exports(&self) -> Option<Vec<String>> {
        // Return exports that were called in the chain
        Some(self.exports_called.iter().cloned().collect())
    }
}
```

### How It Works

1. **Parse chain** → Extract which interfaces were used, which functions were called
2. **Register with HandlerRegistry** → ReplayHandler is the ONLY handler registered
3. **setup_host_functions()** → Create stubs for all imports from chain data
4. **start()** → Iterate chain events, call functions via `actor_handle`, compare hashes

### Manifest Extension

```toml
name = "my-actor"
version = "0.1.0"
component = "./my-actor.wasm"

[replay]
chain = "abc123..."           # Chain ID or path to replay against
mode = "verify"               # "verify" | "debug"
stop_on_divergence = true     # Stop immediately when hashes differ
```

When `[replay]` is present, the CLI registers only the ReplayHandler (no other handlers).

### Chain Data Requirements

The chain must be self-describing. It needs:

#### 1. Interface Definitions (recorded at handler setup)

When a handler registers its host functions, it records the WIT type definitions:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WitType {
    Bool, Char, String,
    U8, U16, U32, U64, S8, S16, S32, S64,
    Float32, Float64,
    List(Box<WitType>),
    Record(Vec<(String, WitType)>),
    Tuple(Vec<WitType>),
    Variant(Vec<(String, Option<WitType>)>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Option(Box<WitType>),
    Result { ok: Option<Box<WitType>>, err: Option<Box<WitType>> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSignature {
    pub name: String,
    pub params: Vec<(String, WitType)>,
    pub results: Vec<WitType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceDefinition {
    pub name: String,  // e.g., "theater:simple/http-client"
    pub functions: Vec<FunctionSignature>,
}
```

Handlers extract this from `component.component_type().imports()` which provides `ComponentItem::ComponentFunc` with full type info.

#### 2. Full I/O Recording (recorded on each host call)

Each host function call records the complete input and output:

```rust
HostFunctionCall {
    interface: String,           // "theater:simple/http-client"
    function: String,            // "send-http"
    input: Vec<u8>,              // Serialized input (matches WIT params)
    output: Vec<u8>,             // Serialized output (matches WIT results)
}
```

The ReplayHandler uses the WIT types to construct Rust types that structurally match, then deserializes the recorded I/O.

#### Current Gap

Events like `HttpClientRequestResult` only capture summaries for logging, not full I/O for replay. Handlers need to be updated to record complete serialized input/output.

### Divergence Detection

```rust
pub struct ReplayResult {
    pub status: ReplayStatus,
    pub events_replayed: usize,
    pub divergence_point: Option<DivergenceInfo>,
}

pub struct DivergenceInfo {
    pub event_index: usize,
    pub original_hash: Vec<u8>,
    pub replay_hash: Vec<u8>,
    pub event_type: String,
}

pub enum ReplayStatus {
    Verified,           // All hashes match
    Diverged,           // Hashes differ at some point
    IncompatibleChain,  // Chain uses interfaces not in component
    Error(String),
}
```

### CLI Integration

```bash
# Verify a chain against a component
theater replay --chain abc123 --component ./my-actor.wasm

# With a manifest that has [replay] section
theater replay --manifest ./replay-manifest.toml
```

## Implementation Phases

### Phase 1: Chain Enhancement
- [ ] Add `replay_data: Option<Vec<u8>>` to event types that need it
- [ ] Update http-client handler to populate replay_data with full response
- [ ] Create chain parser that extracts interfaces and replay data

### Phase 2: ReplayHandler Core
- [ ] Create `ReplayHandler` struct implementing `Handler<E>`
- [ ] Implement `setup_host_functions()` - stub generation from chain
- [ ] Implement `start()` - event iteration and function replay
- [ ] Implement hash comparison logic

### Phase 3: CLI Integration
- [ ] Add `[replay]` section to manifest parsing
- [ ] Add `theater replay` subcommand
- [ ] Wire up ReplayHandler registration when replay mode detected

### Phase 4: Expand Coverage
- [ ] Add replay_data to remaining handlers (filesystem, timing, etc.)
- [ ] Handle edge cases (errors, timeouts, etc.)
- [ ] Documentation and examples

## Open Questions

1. **Timing**: Execute as fast as possible, or preserve original timing?
2. **Non-determinism**: How to handle actors with intentional randomness?
3. **Multi-actor**: How to handle supervisor/child relationships?
4. **Partial replay** (future): Could mix ReplayHandler with real handlers?

## References

- Handler trait: `crates/theater/src/handler/mod.rs`
- Chain implementation: `crates/theater/src/chain/mod.rs`
- Event recording: `crates/theater/src/actor/store.rs`
- Example handler: `crates/theater-handler-http-client/src/lib.rs`
