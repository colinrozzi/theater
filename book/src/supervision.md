# Supervision in Theater

Theater implements a robust supervision system that enables parent actors to manage and monitor their children. This documentation describes the current implementation and planned enhancements.

## Supervision Architecture

In Theater's supervision model:
- Actors exist in a hierarchy
- Parent actors supervise child actors
- Parents can spawn, stop, and restart children
- Parents have access to children's state and event history
- All supervision actions are recorded in the hash chain

### Key Components

The supervision system is implemented through several key components:

1. **TheaterRuntime** (`theater_runtime.rs`):
   - Tracks parent-child relationships
   - Routes supervision commands to appropriate actors
   - Manages actor processes and mailboxes
   - Handles actor lifecycle events

2. **Supervisor WIT Interface** (`supervisor.wit`):
   - Defines the API for parent-child interactions
   - Exposes child management functions
   - Enables access to child state and events

3. **ActorProcess** data structure:
   - Contains actor metadata
   - Tracks children IDs
   - Maintains actor status information
   - Links to actor runtime process

## Current Implementation

The current supervision implementation provides:

### Supervisor Interface

The WebAssembly Interface Type (WIT) definition in `supervisor.wit`:

```wit
interface supervisor {
    // Spawn a new child actor
    spawn: func(manifest: string) -> result<string, string>;
    
    // Get list of child IDs
    list-children: func() -> list<string>;
    
    // Stop a specific child
    stop-child: func(child-id: string) -> result<_, string>;
    
    // Restart a specific child
    restart-child: func(child-id: string) -> result<_, string>;
    
    // Get latest state of a child
    get-child-state: func(child-id: string) -> result<list<u8>, string>;
    
    // Get event history of a child
    get-child-events: func(child-id: string) -> result<list<chain-event>, string>;

    record chain-event {
        hash: list<u8>,
        parent-hash: option<list<u8>>,
        event-type: string,
        data: list<u8>,
        timestamp: u64
    }
}
```

### TheaterRuntime Implementation

The `TheaterRuntime` handles supervision through:

1. **Actor Spawning** (`spawn_actor`):
   - Creates a new actor from a manifest
   - Assigns a unique Theater ID
   - Establishes parent-child relationship
   - Initializes actor runtime and mailbox

2. **Child Management**:
   - `stop_actor`: Terminates an actor and its children
   - `restart_actor`: Stops and restarts an actor
   - `get_actor_state`: Retrieves current actor state
   - `get_actor_events`: Retrieves actor event history

3. **Event Handling**:
   - `handle_actor_event`: Processes events from actors
   - Forwards events to parent actors
   - Updates supervision relationships

### Parent-Child Message Flow

When a parent interacts with a child:

1. Parent invokes a supervisor function
2. Function call is translated to a `TheaterCommand`
3. Command is processed by `TheaterRuntime`
4. Operations are performed on the child actor
5. Results are returned to the parent
6. State changes are recorded in both parent and child chains

### Actor Process Management

Each actor process contains:
- `actor_id`: Unique identifier
- `process`: JoinHandle to the actor runtime
- `mailbox_tx`: Channel for sending messages
- `children`: Set of child actor IDs
- `status`: Current actor status
- `manifest_path`: Path to actor's manifest

## Usage Examples

### Spawning a Child Actor

```rust
use theater::supervisor;

// From a parent actor
fn spawn_child() -> Result<String, String> {
    // Path to child manifest file
    let manifest_path = "child_actor.toml";
    
    // Spawn the child
    let child_id = supervisor::spawn(manifest_path)?;
    
    // Return the child ID
    Ok(child_id)
}
```

### Managing Child Actors

```rust
// List all children
let children = supervisor::list_children();

// Stop a child
supervisor::stop_child(&child_id)?;

// Restart a child
supervisor::restart_child(&child_id)?;

// Get child state
let state = supervisor::get_child_state(&child_id)?;

// Get child event history
let events = supervisor::get_child_events(&child_id)?;
```

## Actor Lifecycle

The supervision system tracks actor status through:

1. **ActorStatus Enum**:
   - `Running`: Actor is active and processing messages
   - `Stopped`: Actor has been terminated
   - `Failed`: Actor has encountered an error

2. **Lifecycle Events**:
   - Actor created (spawn)
   - Actor terminated (stop)
   - Actor restarted (restart)
   - Actor status changed

## Error Handling

The current implementation handles errors through:

1. **Result Types**:
   - All supervision functions return `Result<T, String>`
   - Errors are propagated to the parent actor
   - Error messages are logged for debugging

2. **Status Tracking**:
   - Actor status is updated on errors
   - Parents can monitor child status
   - Restart decisions based on status

## Planned Enhancements

Future versions will expand the supervision system with:

1. **Advanced Restart Strategies**:
   - Different policies for handling errors
   - Exponential backoff options
   - Maximum retry limits
   - Custom restart state options

2. **Supervision Groups**:
   - One-for-one strategy (restart only failed child)
   - All-for-one strategy (restart all children on any failure)
   - Mixed strategies for different groups

3. **Error Classification**:
   - Permanent vs. temporary errors
   - Expected vs. unexpected errors
   - Error pattern recognition
   - Graduated response strategies

4. **Event Filtering**:
   - Selective event propagation
   - Event transformation
   - Context-aware event handling
   - Event correlation

## Best Practices

1. **Actor Organization**
   - Group related functionality in actor hierarchies
   - Isolate failure domains with supervision boundaries
   - Use dedicated supervisor actors for complex management
   - Document parent-child relationships

2. **Error Management**
   - Define clear error handling strategies
   - Consider state consistency on restarts
   - Log important supervision events
   - Monitor restart patterns

3. **State Considerations**
   - Verify state after restarts
   - Consider state dependencies between actors
   - Design for recovery from partial failures
   - Test restart scenarios thoroughly

4. **Performance**
   - Monitor supervision overhead
   - Balance hierarchy depth
   - Consider message volume in parent-child relationships
   - Optimize event propagation patterns
