# Theater Supervisor Handler

Child actor supervision and management handler for the Theater WebAssembly runtime.

## Overview

The Supervisor Handler enables parent actors to spawn, manage, and monitor child actors in the Theater system. It provides comprehensive child lifecycle management with automatic notification of child errors, exits, and external stops.

## Features

- **Child Actor Spawning**: Create new child actors from manifests
- **State Management**: Resume child actors from saved state
- **Lifecycle Monitoring**: Automatic notifications for child events
- **Child Control**: Restart and stop child actors
- **Introspection**: Query child state and event chains
- **Complete Auditability**: All operations recorded in event chains

## Operations

### Child Creation

- `spawn` - Create a new child actor from a manifest file
- `resume` - Resume a child actor from saved state

### Child Management

- `list-children` - Get IDs of all child actors
- `restart-child` - Restart a specific child actor
- `stop-child` - Stop a specific child actor

### Child Introspection

- `get-child-state` - Retrieve the current state of a child actor
- `get-child-events` - Get the complete event chain for a child actor

### Export Functions (Callbacks)

Supervisor actors must implement these functions to receive notifications:

- `handle-child-error` - Called when a child actor encounters an error
- `handle-child-exit` - Called when a child actor exits successfully
- `handle-child-external-stop` - Called when a child actor is stopped externally

## Configuration

```rust
use theater_handler_supervisor::SupervisorHandler;
use theater::config::actor_manifest::SupervisorHostConfig;
use theater::config::permissions::SupervisorPermissions;

let config = SupervisorHostConfig {};
let permissions = Some(SupervisorPermissions::default());
let handler = SupervisorHandler::new(config, permissions);
```

## Child Actor Lifecycle

### 1. Spawning a Child

```wit
// In actor code
let child_id = spawn("path/to/child/manifest.toml", some(init_bytes));
```

This:
- Loads the child's manifest
- Initializes the child with optional init bytes
- Registers the child with the supervisor
- Returns the child's unique ID

### 2. Monitoring Children

The supervisor automatically receives notifications when:
- A child encounters an error → `handle-child-error` is called
- A child exits successfully → `handle-child-exit` is called
- A child is stopped externally → `handle-child-external-stop` is called

### 3. Restarting Children

```wit
// Restart a child that has stopped
let result = restart-child(child_id);
```

This:
- Recreates the child actor from its original manifest
- Starts with fresh state (not the old state)
- Reregisters with the supervisor

### 4. Stopping Children

```wit
// Gracefully stop a child
let result = stop-child(child_id);
```

This:
- Sends shutdown signal to child
- Waits for child to complete
- Calls `handle-child-exit` when done

## Resuming from State

Unlike `spawn`, which creates a fresh actor, `resume` recreates an actor from saved state:

```wit
// Resume a child from saved state
let saved_state = get-child-state(old_child_id);
let new_child_id = resume("path/to/manifest.toml", saved_state);
```

This enables:
- Persisting actor state across restarts
- Checkpointing long-running computations
- Fault tolerance and recovery

## Getting Child Information

### Query Current State

```wit
let state_bytes = get-child-state(child_id);
match state_bytes {
    some(bytes) => // Child has state
    none => // Child has no state or doesn't exist
}
```

### Query Event History

```wit
let events = get-child-events(child_id);
// Returns the complete event chain for introspection
```

## Error Handling

All operations return `result<T, string>` for proper error handling:

```wit
match spawn("manifest.toml", none) {
    ok(child_id) => // Success
    err(message) => // Handle error
}
```

Common errors:
- Manifest file not found
- Invalid manifest format
- Child ID not found
- Permission denied

## Event Recording

Every operation records detailed events:

- **Spawn Events**: Child creation attempts and results
- **Resume Events**: Child restoration from state
- **Lifecycle Events**: Stops, restarts, errors, exits
- **Query Events**: State and event chain requests
- **Error Events**: Detailed error information

## Architecture

### Parent-Child Communication

The supervisor uses channels to receive notifications from children:

1. When spawning/resuming, the supervisor's channel sender is registered with the child
2. When child events occur (error, exit, external stop), they're sent to this channel
3. The supervisor's background task receives these events
4. The supervisor calls the appropriate handler function on the parent actor

### Thread Safety

- Uses `Arc<Mutex<Option<Receiver>>>` for channel management
- Only the original (not cloned) handler instance runs the background task
- All operations are Send + Sync safe

## Example Usage in Actor Manifests

```toml
[[handlers]]
type = "supervisor"
```

## Development

Run tests:
```bash
cargo test -p theater-handler-supervisor
```

Build:
```bash
cargo build -p theater-handler-supervisor
```

## Migration Status

This handler was migrated from the core `theater` crate (`src/host/supervisor.rs`) to provide:

- ✅ Better modularity and separation of concerns
- ✅ Independent testing and development
- ✅ Clearer architecture and boundaries
- ✅ Simplified dependencies

**Original**: 1079 lines in `theater/src/host/supervisor.rs`
**Migrated**: ~1230 lines in standalone crate (includes comprehensive docs)

## License

See the LICENSE file in the repository root.
