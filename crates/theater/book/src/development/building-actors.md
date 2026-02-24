# Building Actors in Theater

This guide walks you through creating actors in Theater using the modern pack_guest system.

## Quick Start

Create a new actor project:

```bash
cargo new --lib my-actor
cd my-actor
```

Add dependencies to Cargo.toml:
```toml
[package]
name = "my-actor"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
pack-guest = { path = "../path/to/pack-guest" }  # Or from git
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "s"
lto = true
```

## Project Structure

```
my-actor/
├── Cargo.toml              # Project configuration
├── actor.toml              # Actor manifest
├── actor.types             # Interface declarations (optional)
└── src/
    └── lib.rs              # Actor implementation
```

## Basic Actor Implementation

Here's a complete example of a simple actor:

```rust
// src/lib.rs
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use pack_guest::{export, import, pack_types, Value, ValueType};

// Set up allocator and panic handler
pack_guest::setup_guest!();

// Embed interface metadata for hash verification
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}

// Import functions from the host
#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

// Export the init function
#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    // Extract state from input tuple
    let state = match input {
        Value::Tuple(items) if !items.is_empty() => items.into_iter().next().unwrap(),
        _ => return err_result("Invalid input format"),
    };

    log(String::from("Actor initialized!"));

    // Return state wrapped in Ok(tuple(state))
    ok_state(state)
}

// Helper functions for Result types
fn err_result(msg: &str) -> Value {
    Value::Result {
        ok_type: ValueType::Tuple(vec![]),
        err_type: ValueType::String,
        value: Err(alloc::boxed::Box::new(Value::String(String::from(msg)))),
    }
}

fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(inner)),
    }
}
```

## Interface Declarations

### Inline Declarations

For simple actors, declare interfaces directly in the `pack_types!` macro:

```rust
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
            send: func(actor-id: string, msg: list<u8>) -> result<_, string>,
            request: func(actor-id: string, msg: list<u8>) -> result<list<u8>, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}
```

### File-Based Declarations

For cleaner code, use a separate `actor.types` file:

**actor.types:**
```
// TCP Echo Actor - Interface Declarations
imports {
    theater:simple/runtime {
        log: func(msg: string),
    }
    theater:simple/tcp {
        listen: func(address: string) -> result<string, string>,
        send: func(connection-id: string, data: list<u8>) -> result<u64, string>,
        receive: func(connection-id: string, max-bytes: u32) -> result<list<u8>, string>,
        close: func(connection-id: string) -> result<_, string>,
    }
}
exports {
    theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    theater:simple/tcp-client.handle-connection: func(state: option<list<u8>>, params: tuple<string>) -> result<tuple<option<list<u8>>>, string>,
}
```

**lib.rs:**
```rust
use pack_guest::{pack_types, import, export};

// Load from file
pack_types!(file = "actor.types");

#[import(module = "theater:simple/runtime", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/tcp", name = "listen")]
fn tcp_listen(address: String) -> Result<String, String>;

// ... rest of implementation
```

## Actor Manifest

Configure your actor in `actor.toml`:

```toml
name = "my-actor"
component_path = "target/wasm32-unknown-unknown/release/my_actor.wasm"

[interface]
implements = "theater:simple/actor"
requires = []

[[handlers]]
type = "runtime"

[[handlers]]
type = "message-server"

[logging]
level = "debug"
```

## Working with the Value Type

The `Value` type is the universal data format in pack_guest. Here's how to work with it:

### Extracting Data

```rust
fn handle_message(input: Value) -> Value {
    // Input is typically a tuple of (state, params)
    let (state, params) = match input {
        Value::Tuple(mut items) if items.len() == 2 => {
            let state = items.remove(0);
            let params = items.remove(0);
            (state, params)
        }
        _ => return err_result("Invalid input"),
    };

    // Extract bytes from Option<List<u8>>
    let state_bytes: Option<Vec<u8>> = match state {
        Value::Option { value: Some(inner), .. } => {
            match *inner {
                Value::List(items, _) => {
                    Some(items.into_iter().filter_map(|v| {
                        if let Value::U8(b) = v { Some(b) } else { None }
                    }).collect())
                }
                _ => None,
            }
        }
        _ => None,
    };

    // Parse state as JSON
    if let Some(bytes) = state_bytes {
        if let Ok(data) = serde_json::from_slice::<MyState>(&bytes) {
            // Process data...
        }
    }

    ok_state(state)
}
```

### Creating Values

```rust
// Create a list of bytes
fn bytes_to_value(data: &[u8]) -> Value {
    Value::List(
        data.iter().map(|b| Value::U8(*b)).collect(),
        ValueType::U8
    )
}

// Create an optional bytes value
fn optional_bytes(data: Option<&[u8]>) -> Value {
    match data {
        Some(bytes) => Value::Option {
            inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
            value: Some(alloc::boxed::Box::new(bytes_to_value(bytes))),
        },
        None => Value::Option {
            inner_type: ValueType::List(alloc::boxed::Box::new(ValueType::U8)),
            value: None,
        },
    }
}
```

## Adding Handler Capabilities

### Message Server Handler

Enable inter-actor messaging:

```rust
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
            send: func(actor-id: string, msg: list<u8>) -> result<_, string>,
            request: func(actor-id: string, msg: list<u8>) -> result<list<u8>, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-request: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>, list<u8>>, string>,
    }
}

#[import(module = "theater:simple/message-server-host", name = "register")]
fn register() -> Result<(), String>;

#[import(module = "theater:simple/message-server-host", name = "send")]
fn send_message(actor_id: String, msg: Vec<u8>) -> Result<(), String>;

#[import(module = "theater:simple/message-server-host", name = "request")]
fn request(actor_id: String, msg: Vec<u8>) -> Result<Vec<u8>, String>;

#[export(name = "theater:simple/message-server-client.handle-send")]
fn handle_send(input: Value) -> Value {
    // Handle incoming message...
}
```

### TCP Handler

Enable TCP networking:

```rust
#[import(module = "theater:simple/tcp", name = "listen")]
fn tcp_listen(address: String) -> Result<String, String>;

#[import(module = "theater:simple/tcp", name = "send")]
fn tcp_send(connection_id: String, data: Vec<u8>) -> Result<u64, String>;

#[import(module = "theater:simple/tcp", name = "receive")]
fn tcp_receive(connection_id: String, max_bytes: u32) -> Result<Vec<u8>, String>;

#[import(module = "theater:simple/tcp", name = "close")]
fn tcp_close(connection_id: String) -> Result<(), String>;

#[export(name = "theater:simple/tcp-client.handle-connection")]
fn handle_connection(input: Value) -> Value {
    // Extract connection_id from params and handle connection...
}
```

### Store Handler

Enable content-addressable storage:

```rust
#[import(module = "theater:simple/store", name = "new")]
fn store_new() -> Result<String, String>;

#[import(module = "theater:simple/store", name = "store")]
fn store_content(store_id: String, content: Vec<u8>) -> Result<String, String>;

#[import(module = "theater:simple/store", name = "get")]
fn store_get(store_id: String, content_ref: String) -> Result<Vec<u8>, String>;

#[import(module = "theater:simple/store", name = "store-at-label")]
fn store_at_label(store_id: String, label: String, content: Vec<u8>) -> Result<String, String>;
```

## Building Your Actor

Build for WebAssembly:

```bash
cargo build --target wasm32-unknown-unknown --release
```

The output will be at `target/wasm32-unknown-unknown/release/my_actor.wasm`.

## Running Your Actor

Run with the Theater CLI:

```bash
theater run actor.toml
```

Or spawn from another actor using the supervisor handler.

## State Management Best Practices

1. **Use Serde for Serialization**
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct State {
    count: i32,
    last_updated: String,
}
```

2. **Handle Errors Gracefully**
```rust
fn parse_state(state_value: Value) -> Option<State> {
    let bytes = extract_bytes_from_option(state_value)?;
    serde_json::from_slice(&bytes).ok()
}
```

3. **Return Proper Result Values**
```rust
// Always return the expected Result<tuple<...>, string> format
fn ok_state(state: Value) -> Value {
    let inner = Value::Tuple(vec![state]);
    Value::Result {
        ok_type: inner.infer_type(),
        err_type: ValueType::String,
        value: Ok(alloc::boxed::Box::new(inner)),
    }
}
```

## Development Tips

1. Use `log()` liberally during development
2. Test with different message types
3. Verify interface hashes match handler expectations
4. Handle all error cases properly
5. Keep state serializable and versioned

## Common Pitfalls

1. **Interface Hash Mismatch**
   - Ensure `pack_types!` declarations exactly match handler expectations
   - Check function names, parameter types, and return types

2. **Forgetting `#![no_std]`**
   - WASM actors should use `no_std` for minimal binary size
   - Use `alloc` crate for heap allocations

3. **Incorrect Value Type Handling**
   - Always match on the expected Value variant
   - Handle unexpected variants gracefully

4. **Missing `pack_guest::setup_guest!()`**
   - Required to set up allocator and panic handler
   - Must be called once at module level

## See Also

- [Pact Interface Definitions](concepts/pact-interfaces.md) - Interface declaration syntax
- [Handler System](../services/handlers/README.md) - Available handlers
- [Testing with Replay](testing-with-replay.md) - Deterministic testing
