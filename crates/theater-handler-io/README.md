# theater-handler-io

WASI I/O handler for Theater providing stream types, error handling, poll mechanisms, and CLI interfaces.

## Overview

This handler implements the core WASI I/O interfaces that enable WebAssembly components to perform input/output operations. It's a foundational handler that many other handlers depend on for stream-based communication.

## Interfaces Provided

### WASI I/O (wasi:io@0.2.3)

- **wasi:io/error** - Error resource type for I/O operations
- **wasi:io/streams** - Input and output stream resources with blocking and non-blocking operations
- **wasi:io/poll** - Pollable resources for async I/O subscriptions

### WASI CLI (wasi:cli@0.2.3)

- **wasi:cli/stdin** - Standard input stream access
- **wasi:cli/stdout** - Standard output stream access  
- **wasi:cli/stderr** - Standard error stream access
- **wasi:cli/environment** - Environment variable access
- **wasi:cli/exit** - Process exit functionality
- **wasi:cli/terminal-input** - Terminal input capabilities
- **wasi:cli/terminal-output** - Terminal output capabilities
- **wasi:cli/terminal-stdin** - Terminal stdin wrapper
- **wasi:cli/terminal-stdout** - Terminal stdout wrapper
- **wasi:cli/terminal-stderr** - Terminal stderr wrapper

## Architecture

### Stream Implementation

Streams are backed by in-memory buffers and provide non-blocking I/O operations. Key types:

- `InputStream` - Readable byte stream with `read`, `blocking-read`, `skip`, `subscribe` methods
- `OutputStream` - Writable byte stream with `write`, `blocking-write-and-flush`, `flush`, `subscribe` methods
- `Pollable` - Resource for waiting on I/O readiness

### Event Recording

The handler records I/O events for audit purposes:
- Stream creation/destruction
- Read/write operations with byte counts
- Poll operations

## Usage

### In Actor Manifests

The IO handler is automatically activated when a WASM component imports any of the interfaces it provides. You don't typically need to explicitly configure it:

```toml
name = "my-actor"
version = "0.1.0"
component = "path/to/component.wasm"

[[handler]]
type = "runtime"
```

### In Test Actors

Create a WIT world that imports the interfaces:

```wit
package my:actor;

world my-actor {
    import wasi:io/error@0.2.3;
    import wasi:io/streams@0.2.3;
    import wasi:io/poll@0.2.3;
    export theater:simple/actor;
}
```

### Direct Handler Usage

When building custom handler registries:

```rust
use theater_handler_io::WasiIoHandler;
use theater::handler::HandlerRegistry;

let mut registry = HandlerRegistry::new();
registry.register(WasiIoHandler::new());
```

## Dependencies

This handler typically works alongside:
- `theater-handler-timing` - For clock-based poll timeouts
- `theater-handler-filesystem` - For file-based streams

## Testing

### Build Test Actor

```bash
cd test-actors/wasi-io-test
cargo component build --release
```

### Run Integration Tests

```bash
cargo test --test integration_test -- --nocapture
```

## WIT Definitions

The handler uses wasmtime's bindgen to generate type-safe Host traits from the WASI I/O and CLI WIT definitions, located in `wit/`.

## Module Structure

- `lib.rs` - Handler implementation and trait impls
- `streams.rs` - InputStream/OutputStream implementations
- `error.rs` - IoError types
- `poll.rs` - Pollable resource implementation
- `events.rs` - Event data types for audit trail
- `bindings.rs` - Generated wasmtime bindings
- `host_impl.rs` - Host trait implementations

## Events

The handler emits `IoEventData` events:

```rust
pub enum IoEventData {
    StreamRead { bytes: usize },
    StreamWrite { bytes: usize },
    StreamSubscribe,
    PollReady { ready: bool },
    // ...
}
```

## License

MIT OR Apache-2.0
