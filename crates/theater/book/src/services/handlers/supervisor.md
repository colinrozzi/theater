# Supervisor Handler

The Supervisor Handler enables parent-child relationships between actors in Theater. It provides the foundation for actor supervision hierarchies, allowing parent actors to spawn, monitor, and control child actors while maintaining the Theater security and verification model.

## Overview

The Supervisor Handler implements the `ntwk:theater/supervisor` interface, enabling actors to:

1. Spawn new child actors
2. Monitor child actor lifecycle events
3. Access child actor state and events
4. Stop and restart child actors
5. Implement supervision strategies for fault tolerance

## Configuration

To use the Supervisor Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "supervisor"
config = {}
```

The Supervisor Handler doesn't currently require any specific configuration parameters.

## Interface

The Supervisor Handler is defined using the following WIT interface:

```wit
interface supervisor {
    // Spawn a new child actor from a manifest
    spawn: func(manifest: string) -> result<string, string>;
    
    // List all child actor IDs
    list-children: func() -> list<string>;
    
    // Stop a specific child actor
    stop-child: func(child-id: string) -> result<_, string>;
    
    // Restart a specific child actor
    restart-child: func(child-id: string) -> result<_, string>;
    
    // Get the current state of a child actor
    get-child-state: func(child-id: string) -> result<list<u8>, string>;
    
    // Get the event history of a child actor
    get-child-events: func(child-id: string) -> result<list<chain-event>, string>;
    
    // Record structure for chain events
    record chain-event {
        hash: list<u8>,
        parent-hash: option<list<u8>>,
        event-type: string,
        data: list<u8>,
        timestamp: u64
    }
}
```

## Spawning Child Actors

The most fundamental operation in the supervision system is spawning child actors. This is done using the `spawn` function:

```rust
// Manifest can be a path to a TOML file or the TOML content as a string
let manifest = r#"
name = "child-actor"
component_path = "child_actor.wasm"

[[handlers]]
type = "message-server"
config = {}
"#;

match supervisor::spawn(manifest) {
    Ok(child_id) => {
        println!("Spawned child actor with ID: {}", child_id);
        // Store the child ID for future reference
    },
    Err(error) => {
        println!("Failed to spawn child actor: {}", error);
    }
}
```

## Managing Child Actors

### Listing Children

To get a list of all child actors:

```rust
let children = supervisor::list_children();
println!("Child actors: {:?}", children);
```

### Stopping a Child

To gracefully stop a child actor:

```rust
match supervisor::stop_child(child_id) {
    Ok(_) => {
        println!("Child actor stopped successfully");
    },
    Err(error) => {
        println!("Failed to stop child actor: {}", error);
    }
}
```

### Restarting a Child

To restart a child actor (useful after failures):

```rust
match supervisor::restart_child(child_id) {
    Ok(_) => {
        println!("Child actor restarted successfully");
    },
    Err(error) => {
        println!("Failed to restart child actor: {}", error);
    }
}
```

## Accessing Child State and Events

One of the powerful features of the supervision system is the ability to access child actor state and event history.

### Getting Child State

To get the current state of a child actor:

```rust
match supervisor::get_child_state(child_id) {
    Ok(state_bytes) => {
        // Process the child state
        if let Some(bytes) = state_bytes {
            let state: ChildState = serde_json::from_slice(&bytes)
                .expect("Failed to deserialize child state");
            println!("Child state: {:?}", state);
        } else {
            println!("Child has no state");
        }
    },
    Err(error) => {
        println!("Failed to get child state: {}", error);
    }
}
```

### Getting Child Events

To get the event history of a child actor:

```rust
match supervisor::get_child_events(child_id) {
    Ok(events) => {
        println!("Child has {} events", events.len());
        
        for event in events {
            println!("Event type: {}", event.event_type);
            println!("Timestamp: {}", event.timestamp);
            
            // Process event data based on type
            // ...
        }
    },
    Err(error) => {
        println!("Failed to get child events: {}", error);
    }
}
```

## Supervision Strategies

The Supervisor Handler enables the implementation of different supervision strategies inspired by the Erlang/OTP model:

### One-for-One Strategy

Restart only the failed child:

```rust
fn handle_child_failure(child_id: &str) -> Result<(), String> {
    // Attempt to restart the failed child
    supervisor::restart_child(child_id)
}
```

### All-for-One Strategy

Restart all children when one fails:

```rust
fn handle_child_failure(failed_child_id: &str) -> Result<(), String> {
    // Get all children
    let children = supervisor::list_children();
    
    // Restart all children
    for child_id in children {
        supervisor::restart_child(&child_id)?;
    }
    
    Ok(())
}
```

### Rest-for-One Strategy

Restart the failed child and all children that depend on it:

```rust
fn handle_child_failure(failed_child_id: &str) -> Result<(), String> {
    // Get dependency tree (implementation specific)
    let dependent_children = get_dependent_children(failed_child_id);
    
    // Restart the failed child first
    supervisor::restart_child(failed_child_id)?;
    
    // Then restart dependent children
    for child_id in dependent_children {
        supervisor::restart_child(&child_id)?;
    }
    
    Ok(())
}
```

## State Chain Integration

All supervision operations are recorded in the parent actor's state chain, creating a verifiable history. The chain events include:

1. **SupervisorOperation**: Records details of supervision operations:
   - Operation type (spawn, stop, restart, etc.)
   - Child actor ID
   - Result (success/failure)

2. **ChildLifecycleEvent**: Records child lifecycle events:
   - Child actor ID
   - Event type (started, stopped, crashed, etc.)
   - Timestamp

This integration ensures that all supervision activities are:
- Traceable
- Verifiable
- Reproducible
- Auditable

## Error Handling

The Supervisor Handler provides detailed error information for various failure scenarios:

1. **Spawn Errors**: When child actor creation fails
2. **Stop Errors**: When child actor termination fails
3. **Restart Errors**: When child actor restart fails
4. **Not Found Errors**: When the specified child actor doesn't exist
5. **Access Errors**: When accessing child state or events fails

## Security Considerations

When using the Supervisor Handler, consider the following security aspects:

1. **Child Isolation**: Child actors run in separate WebAssembly sandboxes
2. **State Access Controls**: Only direct parent actors can access child state
3. **Manifest Validation**: Validate manifests before spawning actors
4. **Resource Limits**: Consider setting limits on child actor resource usage
5. **Privilege Separation**: Design actor hierarchies with security in mind

## Implementation Details

Under the hood, the Supervisor Handler:

1. Communicates with the Theater runtime to manage child actors
2. Tracks parent-child relationships in the actor system
3. Routes supervision commands to the appropriate actors
4. Manages actor processes and mailboxes
5. Handles actor lifecycle events
6. Records all supervision activities in the state chain

## Building Supervision Trees

Supervision trees are a powerful pattern for structuring actor systems. Here's how to build a basic supervision tree:

```rust
// Spawn root supervisor actor
fn init() -> Result<(), String> {
    // Spawn worker actors
    let worker1_id = spawn_worker("worker1")?;
    let worker2_id = spawn_worker("worker2")?;
    
    // Spawn supervisor for a group of related workers
    let group_supervisor_id = spawn_group_supervisor()?;
    
    // Store child IDs for future reference
    let mut state = get_current_state();
    state.children = vec![worker1_id, worker2_id, group_supervisor_id];
    update_state(state);
    
    Ok(())
}

// Function to spawn a worker actor
fn spawn_worker(name: &str) -> Result<String, String> {
    let manifest = format!(r#"
        name = "{}"
        component_path = "worker.wasm"

        [[handlers]]
        type = "message-server"
        config = {{}}
    "#, name);
    
    supervisor::spawn(&manifest)
}

// Function to spawn a group supervisor
fn spawn_group_supervisor() -> Result<String, String> {
    let manifest = r#"
        name = "group-supervisor"
        component_path = "supervisor.wasm"

        [[handlers]]
        type = "supervisor"
        config = {}

        [[handlers]]
        type = "message-server"
        config = {}
    "#;
    
    let supervisor_id = supervisor::spawn(manifest)?;
    
    // Send message to initialize the group supervisor
    // This will cause it to spawn its own child workers
    message_server::request(supervisor_id, init_message())?;
    
    Ok(supervisor_id)
}
```

## Best Practices

1. **Hierarchical Design**: Design clear supervision hierarchies
2. **Failure Domains**: Group related actors under the same supervisor
3. **Restart Strategies**: Choose appropriate restart strategies for different components
4. **State Recovery**: Design child actors to recover gracefully from restarts
5. **Error Handling**: Handle supervision errors properly
6. **Monitoring**: Implement monitoring for supervisor decisions
7. **Testing**: Test supervisor behavior with fault injection

## Dynamic Supervision

You can implement dynamic supervision patterns where actors are spawned and managed at runtime:

```rust
// Handle a request to create a new worker
fn handle_create_worker_request(params: CreateWorkerParams) -> Result<WorkerCreatedResponse, String> {
    // Create a manifest dynamically based on parameters
    let manifest = format!(r#"
        name = "{}"
        component_path = "{}"

        [[handlers]]
        type = "message-server"
        config = {{}}
        
        # Additional handlers based on parameters
        {}
    "#, params.name, params.component_path, generate_handler_config(&params));
    
    // Spawn the worker
    let worker_id = supervisor::spawn(&manifest)?;
    
    // Update supervisor state with new worker
    let mut current_state = get_current_state();
    current_state.workers.push(WorkerInfo {
        id: worker_id.clone(),
        name: params.name.clone(),
        created_at: get_current_time(),
    });
    update_state(current_state);
    
    // Return worker ID to requester
    Ok(WorkerCreatedResponse {
        worker_id,
        status: "created".to_string(),
    })
}
```

## Related Topics

- [Message Server Handler](message-server.md) - For actor-to-actor communication
- [Runtime Handler](runtime.md) - For accessing runtime information and operations
- [Store Handler](store.md) - For content-addressable storage
- [State Management](../core-concepts/state-management.md) - For understanding state chain integration
- [Supervision](../core-concepts/supervision.md) - For deeper supervision concepts
