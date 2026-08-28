# Theater Runtime Handler

Provides runtime information and control capabilities to WebAssembly actors in the Theater system.

## Features

This handler allows actors to:
- **Log messages** - Output log messages from within the actor
- **Get state** - Retrieve the current state information
- **Request shutdown** - Gracefully shutdown the actor with optional data

## Usage

Add this handler when creating your Theater runtime:

```rust
use theater_handler_runtime::RuntimeHandler;
use theater::config::actor_manifest::RuntimeHostConfig;

// Create the handler with theater command channel
let runtime_handler = RuntimeHandler::new(
    RuntimeHostConfig {},
    theater_tx.clone(),
    None, // Optional permissions
);

// Register with your handler registry
registry.register(runtime_handler);
```

## WIT Interface

This handler implements the `theater:simple/runtime` interface:

```wit
interface runtime {
    // Log a message from the actor
    log: func(message: string)
    
    // Get the current state
    get-state: func() -> list<u8>
    
    // Request shutdown with optional data
    shutdown: func(data: option<list<u8>>) -> result<_, string>
}
```

## Configuration

The runtime handler accepts:
- `RuntimeHostConfig` - Currently has no configuration options
- `theater_tx` - Channel for sending commands to the Theater runtime
- `RuntimePermissions` - Optional permission constraints (currently unused)

## Implementation Notes

- The `log` function is synchronous and outputs to the tracing logger
- The `get-state` function returns the last event data from the actor store
- The `shutdown` function is async and sends a shutdown command to the theater runtime
- All operations are recorded as events in the actor's chain

## Events

The handler records the following event types:
- `runtime-setup` - Handler initialization
- `theater:simple/runtime/log` - Log operations
- `theater:simple/runtime/get-state` - State retrieval
- `theater:simple/runtime/shutdown` - Shutdown requests
