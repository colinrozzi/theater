# Pact Interface Definitions

Theater uses **Pact** as its interface definition language. Pact provides a concise syntax for declaring the functions that handlers expose to actors (imports) and the functions that actors expose to handlers (exports).

## Overview

Pact files define the contract between actors and the Theater runtime. Each handler (runtime, message-server, store, tcp, etc.) has a corresponding Pact file that specifies:

- **Package**: The namespace for the interface (e.g., `theater:simple`)
- **Interface name**: The specific capability (e.g., `runtime`, `store`)
- **Functions**: The operations available with their signatures

## Pact File Format

A Pact interface file has this structure:

```pact
// Comment describing the interface
interface interface-name {
    @package: string = "namespace:package"

    exports {
        // Functions provided by this interface
        function-name: func(param: type) -> return-type
    }
}
```

### Metadata

The `@package` metadata specifies the full package path. Combined with the interface name, this forms the complete interface identifier:

```pact
interface runtime {
    @package: string = "theater:simple"
    // Full identifier: theater:simple/runtime
}
```

### Type System

Pact supports these types:

| Type | Description | Example |
|------|-------------|---------|
| `string` | UTF-8 string | `msg: string` |
| `bool` | Boolean | `enabled: bool` |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers | `count: u64` |
| `s8`, `s16`, `s32`, `s64` | Signed integers | `offset: s32` |
| `f32`, `f64` | Floating point | `value: f64` |
| `list<T>` | List of T | `data: list<u8>` |
| `option<T>` | Optional T | `state: option<list<u8>>` |
| `result<T, E>` | Result type | `result<string, string>` |
| `tuple<...>` | Tuple | `tuple<string, list<u8>>` |
| `_` | Unit/void (in results) | `result<_, string>` |

### Function Signatures

Functions are declared with name, parameters, and optional return type:

```pact
// No parameters, no return
simple-action: func()

// Parameters, no return
log: func(msg: string)

// Parameters with return
get-data: func(key: string) -> list<u8>

// Fallible operation (Result type)
store: func(data: list<u8>) -> result<string, string>

// Optional return
lookup: func(id: string) -> option<list<u8>>
```

## Example Pact Files

### Runtime Interface

The runtime interface provides core actor capabilities:

```pact
// Theater Runtime Interface
interface runtime {
    @package: string = "theater:simple"

    exports {
        // Log a message
        log: func(msg: string)

        // Get the actor's event chain
        get-chain: func() -> list<u8>

        // Shutdown the actor
        shutdown: func(data: option<list<u8>>) -> result<_, string>
    }
}
```

### Store Interface

Content-addressable storage:

```pact
interface store {
    @package: string = "theater:simple"

    exports {
        // Create a new store
        new: func() -> result<string, string>

        // Store content, get hash reference
        store: func(store-id: string, content: list<u8>) -> result<string, string>

        // Retrieve content by reference
        get: func(store-id: string, content-ref: string) -> result<list<u8>, string>

        // Store with label
        store-at-label: func(store-id: string, label: string, content: list<u8>) -> result<string, string>
    }
}
```

### Message Server Interface

Inter-actor messaging:

```pact
interface message-server-host {
    @package: string = "theater:simple"

    exports {
        // Register to receive messages
        register: func() -> result<_, string>

        // Send one-way message
        send: func(actor-id: string, msg: list<u8>) -> result<_, string>

        // Send request and await response
        request: func(actor-id: string, msg: list<u8>) -> result<list<u8>, string>
    }
}
```

## Using Pact in Actors

Actors declare their interface requirements using the `pack_types!` macro. This embeds interface metadata in the WASM binary for hash verification at runtime.

### Inline Declarations

```rust
use pack_guest::{pack_types, import, export};

// Declare interfaces inline
pack_types! {
    imports {
        theater:simple/runtime {
            log: func(msg: string),
        }
        theater:simple/message-server-host {
            register: func() -> result<_, string>,
            send: func(actor-id: string, msg: list<u8>) -> result<_, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(state: option<list<u8>>) -> result<tuple<option<list<u8>>>, string>,
        theater:simple/message-server-client.handle-send: func(state: option<list<u8>>, params: tuple<string, list<u8>>) -> result<tuple<option<list<u8>>>, string>,
    }
}
```

### File-based Declarations

For cleaner code, declare interfaces in a separate file:

**actor.types:**
```
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

#[export(name = "theater:simple/actor.init")]
fn init(input: Value) -> Value {
    // ...
}
```

## Interface Hash Verification

Theater uses interface hashes for O(1) compatibility checking:

1. **At build time**: `pack_types!` computes a Merkle-tree hash of the interface
2. **At runtime**: The handler computes its expected hash
3. **On spawn**: Hashes are compared - mismatches fail immediately with clear errors

This catches interface mismatches before execution, providing:
- Fast startup (no function-by-function verification)
- Clear error messages showing expected vs actual interfaces
- Version compatibility checking

## Pact File Locations

Theater's handler interfaces are defined in `/pact/`:

```
pact/
├── runtime.pact          # Core runtime (log, shutdown)
├── store.pact            # Content-addressable storage
├── message-server.pact   # Message server host functions
├── message-server-client.pact  # Message handler exports
├── tcp.pact              # TCP networking
├── tcp-client.pact       # TCP handler exports
├── supervisor.pact       # Child actor management
├── rpc.pact              # Direct function calls
└── assembler.pact        # Dynamic actor assembly
```

## Best Practices

1. **Keep interfaces focused**: Each interface should provide a single capability
2. **Use descriptive names**: Function names should clearly indicate their purpose
3. **Document with comments**: Add comments explaining each function's behavior
4. **Use appropriate return types**: Fallible operations should return `result<T, E>`
5. **Prefer `list<u8>` for data**: Bytes are the universal interchange format
6. **Match signatures exactly**: The `pack_types!` declaration must match handler expectations

## Migration from WIT

If you have existing WIT interfaces, the Pact syntax is similar but simpler:

**WIT:**
```wit
interface example {
    record my-data {
        field1: string,
        field2: u32,
    }

    my-func: func(data: my-data) -> result<string, string>;
}
```

**Pact:**
```pact
interface example {
    @package: string = "my:package"

    exports {
        // Records become inline tuple types or are serialized as list<u8>
        my-func: func(data: list<u8>) -> result<string, string>
    }
}
```

Key differences:
- Pact uses `exports {}` block for function declarations
- Pact requires `@package` metadata
- Complex records are typically serialized as `list<u8>` (JSON, MessagePack, etc.)
- Pact focuses on the wire format, not complex type definitions
