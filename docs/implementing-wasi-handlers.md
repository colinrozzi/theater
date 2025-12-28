# Implementing WASI Handlers in Theater

This guide walks through implementing a WASI-compliant handler in Theater, using `wasi:random` as a concrete example.

## Overview

WASI (WebAssembly System Interface) provides standard interfaces for WebAssembly components. Theater handlers can implement these standard interfaces, making actors portable across different WASI runtimes.

## Architecture

Theater's WASI handler implementation involves three main components:

1. **Handler Implementation** - Rust code that implements the WASI interface
2. **WIT Definition** - Interface definition in WebAssembly Interface Types format
3. **Test Actor** - WebAssembly component that imports and uses the interface

## Step 1: Define the WIT Interface

Create a WIT file that defines the WASI interface. This should match the official WASI specification.

**File:** `crates/theater/wit/wasi-random.wit`

```wit
package wasi:random@0.2.8;

/// WASI Random provides cryptographically secure random data.
interface random {
    /// Return `len` cryptographically-secure random or pseudo-random bytes.
    /// This function must produce data at least as cryptographically secure and
    /// fast as an adequately seeded CSPRNG. It must not block.
    get-random-bytes: func(len: u64) -> list<u8>;

    /// Return a cryptographically-secure random or pseudo-random `u64` value.
    get-random-u64: func() -> u64;
}
```

**Key points:**
- Use the official WASI package name (e.g., `wasi:random@0.2.8`)
- Include documentation from the WASI spec
- Match function signatures exactly to the specification

## Step 2: Define Event Types

Add event types to track all operations for the event chain. This is **critical** for correctness and replay.

**File:** `crates/theater-handler-random/src/events.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HandlerEventData {
    // Call events - log when functions are called
    #[serde(rename = "random-bytes-call")]
    RandomBytesCall {
        requested_size: usize,
    },

    // Result events - log actual data returned (for replay)
    #[serde(rename = "random-bytes-result")]
    RandomBytesResult {
        generated_size: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        bytes: Option<Vec<u8>>, // CRITICAL: Store actual bytes for replay
        success: bool,
    },

    #[serde(rename = "random-u64-call")]
    RandomU64Call,

    #[serde(rename = "random-u64-result")]
    RandomU64Result {
        value: u64, // CRITICAL: Store actual value for replay
        success: bool,
    },
}
```

**Event Chain Requirements:**

The event chain is for **correctness and replay**, not just debugging. You must log:

1. **Call events**: Function name + ALL input parameters
2. **Result events**: ALL output/return values (including actual data)
3. **Error events**: Any errors that occur

This ensures the event chain contains enough information to replay actor execution exactly.

## Step 3: Implement the Handler

Implement the WASI interface in the handler's `setup_host_functions` method.

**File:** `crates/theater-handler-random/src/lib.rs`

```rust
use theater::wasm::ActorComponent;
use wasmtime::component::__internal::async_trait;
use std::sync::{Arc, Mutex};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

pub struct RandomHandler {
    config: RandomHandlerConfig,
    rng: Arc<Mutex<ChaCha20Rng>>,
    permissions: Option<RandomPermissions>,
}

impl RandomHandler {
    pub fn new(config: RandomHandlerConfig, permissions: Option<RandomPermissions>) -> Self {
        let rng = if let Some(seed) = config.seed {
            Arc::new(Mutex::new(ChaCha20Rng::seed_from_u64(seed)))
        } else {
            Arc::new(Mutex::new(ChaCha20Rng::from_entropy()))
        };

        Self {
            config,
            rng,
            permissions,
        }
    }

    fn setup_wasi_random<E>(
        &mut self,
        actor_component: &mut ActorComponent<E>,
    ) -> anyhow::Result<()>
    where
        E: EventPayload + Clone + From<HandlerEventData>,
    {
        info!("setup_wasi_random called");

        let rng1 = Arc::clone(&self.rng);
        let rng2 = Arc::clone(&self.rng);

        // Get the wasi:random/random@0.2.3 interface
        // Note: Must match exact version that component imports
        let mut interface = match actor_component.linker.instance("wasi:random/random@0.2.3") {
            Ok(interface) => {
                info!("Successfully got wasi:random/random@0.2.3 instance");
                interface
            }
            Err(e) => {
                info!("Failed to get wasi:random/random@0.2.3: {:?}", e);
                return Ok(()); // Actor doesn't import this interface
            }
        };

        // Implement get-random-bytes: func(len: u64) -> list<u8>
        interface.func_wrap(
            "get-random-bytes",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (len,): (u64,)| -> anyhow::Result<(Vec<u8>,)> {
                let len = len as usize;

                // Log the call
                ctx.data_mut().record_handler_event(
                    "wasi:random/random/get-random-bytes".to_string(),
                    HandlerEventData::RandomBytesCall { requested_size: len },
                    Some(format!("WASI random: requesting {} bytes", len)),
                );

                let mut bytes = vec![0u8; len];
                match rng1.lock() {
                    Ok(mut generator) => {
                        generator.fill_bytes(&mut bytes);

                        // CRITICAL: Log the actual bytes for replay
                        ctx.data_mut().record_handler_event(
                            "wasi:random/random/get-random-bytes".to_string(),
                            HandlerEventData::RandomBytesResult {
                                generated_size: len,
                                bytes: Some(bytes.clone()),
                                success: true,
                            },
                            Some(format!("WASI random: generated {} bytes", len)),
                        );

                        Ok((bytes,))
                    }
                    Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e))
                }
            },
        )?;

        // Implement get-random-u64: func() -> u64
        interface.func_wrap(
            "get-random-u64",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, ()| -> anyhow::Result<(u64,)> {
                ctx.data_mut().record_handler_event(
                    "wasi:random/random/get-random-u64".to_string(),
                    HandlerEventData::RandomU64Call,
                    Some("WASI random: requesting random u64".to_string()),
                );

                match rng2.lock() {
                    Ok(mut generator) => {
                        let value: u64 = generator.gen();

                        // CRITICAL: Log the actual value for replay
                        ctx.data_mut().record_handler_event(
                            "wasi:random/random/get-random-u64".to_string(),
                            HandlerEventData::RandomU64Result {
                                value,
                                success: true
                            },
                            Some(format!("WASI random: generated u64 = {}", value)),
                        );

                        Ok((value,))
                    }
                    Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e))
                }
            },
        )?;

        Ok(())
    }
}

impl<E> Handler<E> for RandomHandler
where
    E: EventPayload + Clone + From<HandlerEventData>,
{
    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
    ) -> anyhow::Result<()> {
        info!("Setting up WASI random host functions");

        // Set up WASI-compliant random interface
        self.setup_wasi_random(actor_component)?;

        info!("WASI random host functions setup complete");
        Ok(())
    }

    fn imports(&self) -> Option<String> {
        // MUST match exact import name including version
        Some("wasi:random/random@0.2.3".to_string())
    }

    fn name(&self) -> &str {
        "random"
    }

    fn exports(&self) -> Option<String> {
        None
    }

    // ... other Handler trait methods
}
```

**Key implementation details:**

1. **Version matching**: The string returned by `imports()` must exactly match what the component imports (including version)
2. **Linker instance name**: Use the full versioned name `"wasi:random/random@0.2.3"`
3. **Function wrapping**: Use `func_wrap` for sync functions, `func_wrap_async` for async
4. **Event logging**: Log both the call AND the result with actual data
5. **Error handling**: Convert handler errors to anyhow::Result

## Step 4: Create a Test Actor

Create a test actor that imports the WASI interface.

### Project Structure

```
test-actors/wasi-random-test/
├── Cargo.toml
├── manifest.toml
├── wit/
│   ├── world.wit
│   └── deps/
│       ├── wasi-random/    # fetched via wkg
│       └── theater-simple/ # fetched via wkg
└── src/
    └── lib.rs
```

### world.wit

**File:** `test-actors/wasi-random-test/wit/world.wit`

```wit
package test:wasi-random;

world wasi-random-test {
    import wasi:random/random@0.2.0;  // Import WASI interface
    export theater:simple/actor;       // Export Theater actor interface
}
```

### Cargo.toml

**File:** `test-actors/wasi-random-test/Cargo.toml`

```toml
[package]
name = "wasi-random-test"
version = "0.1.0"
edition = "2021"

[workspace]  # Important if test actor is in same workspace

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen-rt = { version = "0.43.0", features = ["bitflags"] }

[package.metadata.component]
package = "test:wasi-random"

[package.metadata.component.target.dependencies]
"wasi:random" = { path = "./wit/deps/wasi-random" }
"theater:simple" = { path = "./wit/deps/theater-simple" }

[package.metadata.component.bindings]
derives = ["serde::Serialize", "serde::Deserialize", "PartialEq"]
generate_unused_types = true
```

### Actor Implementation

**File:** `test-actors/wasi-random-test/src/lib.rs`

```rust
mod bindings;

use bindings::wasi::random::random;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(
        _state: Option<Vec<u8>>,
        _params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Test 1: get-random-bytes with small size
        let bytes_10 = random::get_random_bytes(10);
        assert_eq!(bytes_10.len(), 10, "Should generate exactly 10 bytes");

        // Test 2: get-random-bytes with different size
        let bytes_32 = random::get_random_bytes(32);
        assert_eq!(bytes_32.len(), 32, "Should generate exactly 32 bytes");

        // Test 3: get-random-u64
        let _rand_u64_1 = random::get_random_u64();
        let _rand_u64_2 = random::get_random_u64();

        // Test 4: Verify randomness (bytes should not all be zeros)
        let bytes_check = random::get_random_bytes(100);
        let all_zeros = bytes_check.iter().all(|&b| b == 0);
        assert!(!all_zeros, "Random bytes should not all be zero");

        Ok((Some(b"WASI random tests passed!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
```

### manifest.toml

**File:** `test-actors/wasi-random-test/manifest.toml`

```toml
name = "wasi-random-test"
version = "0.1.0"
component = "/Users/colinrozzi/work/theater/test-actors/wasi-random-test/target/wasm32-unknown-unknown/release/wasi_random_test.wasm"
description = "WASI random interface test actor"
save_chain = true

[[handler]]
type = "random"
```

**Note**: Use absolute path for `component` field to avoid path resolution issues.

## Step 5: Build and Run

### Fetch WIT dependencies

```bash
cd test-actors/wasi-random-test
wkg wit fetch
```

This fetches the WASI interface definitions into `wit/deps/`.

### Build the actor

```bash
theater build
```

This compiles the actor to a WASM component targeting `wasm32-unknown-unknown`.

### Start the Theater server

```bash
cargo run -p theater-server-cli --release -- --log-level debug --log-stdout
```

### Run the test actor

```bash
theater start /path/to/test-actors/wasi-random-test/manifest.toml
```

### Verify success

Check the server logs for:
- Handler setup messages
- WASI function calls
- Event chain logging
- Actor initialization completion

Example successful output:
```
INFO theater_handler_random: Setting up WASI random host functions
INFO theater_handler_random: Successfully got wasi:random/random@0.2.3 instance
DEBUG theater::chain: Sending event wasi:random/random/get-random-bytes WASI random: generated 10 bytes
DEBUG theater::chain: Sending event wasi:random/random/get-random-u64 WASI random: generated u64 = 9312200585634548879
DEBUG theater::actor::runtime: Call to 'theater:simple/actor.init' completed, new state size: 25
```

## Common Issues and Solutions

### Issue: "component imports instance `wasi:random/random@0.2.3`, but a matching implementation was not found"

**Cause**: The handler's `imports()` method doesn't return the exact version string.

**Solution**: Update the handler to return the full versioned name:
```rust
fn imports(&self) -> Option<String> {
    Some("wasi:random/random@0.2.3".to_string())
}
```

And use the same version in `linker.instance()`:
```rust
actor_component.linker.instance("wasi:random/random@0.2.3")
```

### Issue: Handler not being activated

**Cause**: Theater's handler selection uses exact string matching. The import name must match exactly.

**Solution**: Check that:
1. The WIT world imports the interface: `import wasi:random/random@0.2.0`
2. The handler returns the matching import (with Theater's resolved version): `wasi:random/random@0.2.3`
3. The linker.instance call uses the same: `"wasi:random/random@0.2.3"`

### Issue: Actor build fails - "bindings module not found"

**Cause**: Missing `mod bindings;` declaration.

**Solution**: Add to the top of `lib.rs`:
```rust
mod bindings;
```

### Issue: WIT dependencies not found

**Cause**: Dependencies not fetched with `wkg wit fetch`.

**Solution**: Run in the actor directory:
```bash
wkg wit fetch
```

## Best Practices

1. **Follow WASI specifications exactly** - Match function signatures, semantics, and behavior from official WASI specs
2. **Log all boundary crossings** - Record every input and output in the event chain for replay capability
3. **Use versioned imports** - Always specify the WASI interface version
4. **Test thoroughly** - Create test actors that exercise all interface functions
5. **Document behavior** - Include comments explaining WASI requirements and Theater-specific details
6. **Handle errors gracefully** - Convert all errors to appropriate WASI error types
7. **Use absolute paths** - In manifest.toml, use absolute paths for the component field

## References

- [WASI 0.2 Interfaces](https://wasi.dev/interfaces)
- [WASI Random Specification](https://github.com/WebAssembly/wasi-random)
- [Component Model Documentation](https://component-model.bytecodealliance.org/)
- [Wasmtime Component Model Guide](https://docs.wasmtime.dev/api/wasmtime/component/)
