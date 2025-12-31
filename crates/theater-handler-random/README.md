# Theater Handler: Random

A WASI-compliant random number generation handler for the Theater WebAssembly actor runtime.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Directory Structure](#directory-structure)
- [How It Works](#how-it-works)
  - [1. WIT Definition](#1-wit-definition)
  - [2. Rust Bindings Generation](#2-rust-bindings-generation)
  - [3. Host Implementation](#3-host-implementation)
  - [4. Handler Registration](#4-handler-registration)
  - [5. Handler Activation](#5-handler-activation)
  - [6. Runtime Execution](#6-runtime-execution)
- [Creating a Test Actor](#creating-a-test-actor)
- [Integration Testing](#integration-testing)
- [Configuration](#configuration)
- [Events](#events)
- [Versioning](#versioning)
- [License](#license)

## Overview

This handler implements the [WASI Random](https://github.com/WebAssembly/wasi-random) interfaces, providing secure random number generation capabilities to WebAssembly actors running in Theater. It supports:

- **`wasi:random/random@0.2.3`** - Cryptographically secure random bytes and u64 values
- **`wasi:random/insecure@0.2.3`** - Fast, non-cryptographic random values  
- **`wasi:random/insecure-seed@0.2.3`** - Seed generation for actor-side PRNGs

All random operations are recorded in the actor's event chain, enabling:
- **Reproducible execution** - Replay actors with deterministic random values
- **Auditing** - Track exactly what random values were generated
- **Debugging** - Understand actor behavior through event inspection

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Theater Runtime                                     │
│                                                                              │
│  ┌─────────────┐    ┌──────────────────┐    ┌─────────────────────────────┐ │
│  │   Actor     │    │  HandlerRegistry │    │     RandomHandler           │ │
│  │  (WASM)     │    │                  │    │                             │ │
│  │             │    │  Matches imports │    │  - Creates RNG per actor    │ │
│  │ imports:    │───▶│  to handlers     │───▶│  - Implements Host traits   │ │
│  │ wasi:random │    │                  │    │  - Records events           │ │
│  │             │    └──────────────────┘    │                             │ │
│  └─────────────┘                            └─────────────────────────────┘ │
│         │                                              │                     │
│         │  get_random_bytes(10)                        │                     │
│         ▼                                              ▼                     │
│  ┌─────────────────────────────────────────────────────────────────────────┐ │
│  │                         ActorStore                                       │ │
│  │  - Holds actor state                                                     │ │
│  │  - Records event chain                                                   │ │
│  │  - Provides context for Host trait implementations                       │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
theater-handler-random/
├── Cargo.toml                    # Crate manifest
├── README.md                     # This file
├── src/
│   ├── lib.rs                    # Handler implementation (Handler trait)
│   ├── bindings.rs               # wasmtime::bindgen! macro invocation
│   ├── host_impl.rs              # Host trait implementations for ActorStore
│   └── events.rs                 # Event data structures
├── wit/
│   ├── world.wit                 # Handler's WIT world definition
│   └── deps/
│       └── wasi-random-0.2.0/    # WASI random WIT package
│           └── package.wit
├── test-actors/
│   └── wasi-random-test/         # Test actor that exercises the handler
│       ├── Cargo.toml
│       ├── manifest.toml
│       ├── src/
│       │   ├── lib.rs            # Actor implementation
│       │   └── bindings.rs       # Generated bindings (by cargo-component)
│       └── wit/
│           ├── world.wit
│           └── deps/
│               ├── wasi-random/
│               └── theater-simple/
└── tests/
    └── integration_test.rs       # Full integration tests
```

## How It Works

### 1. WIT Definition

The handler declares what interfaces it provides in `wit/world.wit`:

```wit
package theater:random-handler;

world random-handler-host {
    /// We provide the WASI random interface to the actor
    import wasi:random/random@0.2.3;
    import wasi:random/insecure@0.2.3;
    import wasi:random/insecure-seed@0.2.3;
}
```

This tells wasmtime's bindgen what Host traits to generate.

### 2. Rust Bindings Generation

In `src/bindings.rs`, we use wasmtime's `bindgen!` macro:

```rust
use wasmtime::component::bindgen;

bindgen!({
    world: "random-handler-host",
    path: "wit",
    async: true,
    trappable_imports: true,
});

// Re-export generated traits
pub use wasi::random::random::Host as RandomHost;
pub use wasi::random::insecure::Host as InsecureHost;
pub use wasi::random::insecure_seed::Host as InsecureSeedHost;
```

This generates traits like:

```rust
#[async_trait]
pub trait RandomHost {
    async fn get_random_bytes(&mut self, len: u64) -> Result<Vec<u8>>;
    async fn get_random_u64(&mut self) -> Result<u64>;
}
```

### 3. Host Implementation

In `src/host_impl.rs`, we implement the generated traits for `ActorStore<E>`:

```rust
impl<E> RandomHost for ActorStore<E>
where
    E: EventPayload + Clone + From<RandomEventData> + Send,
{
    async fn get_random_bytes(&mut self, len: u64) -> Result<Vec<u8>> {
        // 1. Record the call event
        self.record_handler_event(
            "wasi:random/random/get-random-bytes".to_string(),
            RandomEventData::RandomBytesCall { requested_size: len as usize },
            Some(format!("WASI random: requesting {} bytes", len)),
        );

        // 2. Generate random bytes using thread-local RNG
        let rng = get_rng();
        let mut bytes = vec![0u8; len as usize];
        rng.lock().unwrap().fill_bytes(&mut bytes);

        // 3. Record the result event (including actual bytes for replay)
        self.record_handler_event(
            "wasi:random/random/get-random-bytes".to_string(),
            RandomEventData::RandomBytesResult {
                generated_size: len as usize,
                bytes: Some(bytes.clone()),
                success: true,
            },
            Some(format!("WASI random: generated {} bytes", len)),
        );

        Ok(bytes)
    }
    // ... other methods
}
```

### 4. Handler Registration

The handler implements the `Handler<E>` trait in `src/lib.rs`:

```rust
impl<E> Handler<E> for RandomHandler
where
    E: EventPayload + Clone + From<RandomEventData>,
{
    fn name(&self) -> &str {
        "random"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // These must match the component's imports EXACTLY (including version)
        Some(vec![
            "wasi:random/random@0.2.3".to_string(),
            "wasi:random/insecure@0.2.3".to_string(),
            "wasi:random/insecure-seed@0.2.3".to_string(),
        ])
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
        _ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        // Initialize thread-local RNG for this actor
        set_thread_rng(Arc::clone(&self.rng));

        // Add the interfaces to the linker
        bindings::wasi::random::random::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        // ... add other interfaces
        Ok(())
    }
    // ... other trait methods
}
```

### 5. Handler Activation

When an actor is spawned, Theater's `HandlerRegistry::setup_handlers()` matches component imports to handler imports:

```rust
// In theater/src/handler/mod.rs
pub fn setup_handlers(&mut self, actor_component: &mut ActorComponent<E>) -> Vec<Box<dyn Handler<E>>> {
    let component_imports: HashSet<String> = actor_component.import_types
        .iter()
        .map(|(name, _)| name.clone())
        .collect();

    for handler in &self.handlers {
        let handler_imports = handler.imports();
        
        // Check if ANY of handler's imports match component's imports
        let imports_match = handler_imports
            .as_ref()
            .map_or(false, |imports| {
                imports.iter().any(|import| component_imports.contains(import))
            });

        if imports_match {
            active_handlers.push(handler.create_instance());
        }
    }
    active_handlers
}
```

**Important**: The matching is **exact string comparison**. Version numbers matter!
- `wasi:random/random@0.2.3` does NOT match `wasi:random/random@0.2.0`

### 6. Runtime Execution

When the actor calls a WASI function:

1. Wasmtime invokes the Host trait implementation
2. The implementation runs with `ActorStore` as `self`
3. Events are recorded to the chain
4. Results are returned to the actor

## Creating a Test Actor

Test actors live in `test-actors/` and are built as WebAssembly components.

### 1. Create the actor structure

```
test-actors/wasi-random-test/
├── Cargo.toml
├── manifest.toml          # Theater manifest
├── src/lib.rs             # Actor code
└── wit/
    ├── world.wit          # Actor's world
    └── deps/
        ├── wasi-random/   # WASI random WIT
        └── theater-simple/ # Theater actor interface
```

### 2. Define the WIT world (`wit/world.wit`)

```wit
package test:wasi-random;

world wasi-random-test {
    // Import what we need from the host
    import wasi:random/random@0.2.3;
    
    // Export the Theater actor interface
    export theater:simple/actor;
}
```

### 3. Write the actor (`src/lib.rs`)

```rust
mod bindings;
use bindings::wasi::random::random;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(_state: Option<Vec<u8>>, _params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        // Test random bytes
        let bytes = random::get_random_bytes(10);
        assert_eq!(bytes.len(), 10);

        // Test random u64
        let _value = random::get_random_u64();

        Ok((Some(b"WASI random tests passed!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
```

### 4. Configure Cargo.toml

```toml
[package]
name = "wasi-random-test"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen-rt = { version = "0.43.0", features = ["bitflags"] }

[package.metadata.component]
package = "test:wasi-random"

[package.metadata.component.target.dependencies]
"wasi:random" = { path = "./wit/deps/wasi-random" }
"theater:simple" = { path = "./wit/deps/theater-simple" }
```

### 5. Build the actor

```bash
cd test-actors/wasi-random-test
cargo component build --release
```

The WASM component will be at `target/wasm32-wasip1/release/wasi_random_test.wasm`.

## Integration Testing

The integration test (`tests/integration_test.rs`) demonstrates the full handler lifecycle:

```rust
#[tokio::test]
async fn test_random_handler_with_test_actor() -> Result<()> {
    // 1. Define custom event type that wraps handler events
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum TestHandlerEvents {
        Random(RandomEventData),
        // ... other handlers
    }

    // 2. Create handler registry with all needed handlers
    let mut registry = HandlerRegistry::new();
    registry.register(RuntimeHandler::new(config, theater_tx, None));
    registry.register(RandomHandler::new(random_config, None));
    registry.register(WasiIoHandler::new());  // For wasi:cli/*
    registry.register(TimingHandler::new(timing_config, None));  // For wasi:clocks/*
    registry.register(FilesystemHandler::new(fs_config, None));  // For wasi:filesystem/*

    // 3. Create and run Theater runtime
    let mut runtime = TheaterRuntime::new(theater_tx, theater_rx, None, registry).await?;
    tokio::spawn(async move { runtime.run().await });

    // 4. Spawn the test actor
    theater_tx.send(TheaterCommand::SpawnActor {
        manifest_path: manifest_content,
        // ...
    }).await?;

    // 5. Collect and verify events
    // Events include: setup events, init call, random calls, results
}
```

Run with:
```bash
cargo test --package theater-handler-random --test integration_test -- --nocapture
```

## Configuration

```rust
use theater::config::actor_manifest::RandomHandlerConfig;

let config = RandomHandlerConfig {
    seed: Some(12345),           // Fixed seed for reproducibility (None = OS entropy)
    max_bytes: 1024 * 1024,      // Maximum bytes per request
    max_int: u64::MAX - 1,       // Maximum integer value
    allow_crypto_secure: true,   // Allow cryptographic operations
};

let handler = RandomHandler::new(config, None);
```

### Manifest Configuration

In actor manifests (`manifest.toml`):

```toml
[[handler]]
type = "random"
seed = 12345              # Optional: fixed seed
max_bytes = 1048576       # Optional: max bytes per request
max_int = 9223372036854775807  # Optional: max integer
allow_crypto_secure = false    # Optional: crypto permissions
```

## Events

All random operations are recorded as events in the actor's chain:

| Event Type | Description |
|------------|-------------|
| `RandomBytesCall` | Actor requested random bytes |
| `RandomBytesResult` | Random bytes generated (includes actual bytes) |
| `RandomU64Call` | Actor requested random u64 |
| `RandomU64Result` | Random u64 generated (includes value) |
| `InsecureRandomBytesCall/Result` | Insecure random bytes |
| `InsecureRandomU64Call/Result` | Insecure random u64 |
| `InsecureSeedCall/Result` | Seed generation |
| `Error` | Operation failed |
| `PermissionDenied` | Operation not allowed |

Events include the actual random values, enabling deterministic replay.

## Versioning

**Critical**: WASI interface versions must match exactly between:

1. **Handler's `imports()`** - What the handler advertises it provides
2. **WIT files** - What the bindgen generates from  
3. **Actor's imports** - What the WASM component needs

If you see handlers not activating, check version alignment:

```bash
# Check what the WASM component imports
wasm-tools component wit path/to/actor.wasm

# Verify it matches handler's imports() return value
```

Current versions (WASI 0.2.3):
- `wasi:random/random@0.2.3`
- `wasi:random/insecure@0.2.3`
- `wasi:random/insecure-seed@0.2.3`

## License

Apache-2.0
