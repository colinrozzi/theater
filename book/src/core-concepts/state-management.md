# State Management System

## Overview

The State Management system in Theater provides a structured approach to handling actor state throughout the lifecycle of WebAssembly actors. By separating state management from business logic, Theater creates a foundation for powerful capabilities like state introspection, migration, and recovery.

## Core Principles

1. **Runtime-Managed State**: The actor's state is managed by the runtime rather than the actor itself, allowing for greater control and flexibility.

2. **Transparent State Passing**: State is automatically passed as the first argument to handler functions and received as the first result, making state management largely transparent to actor developers.

3. **Immutable State Transitions**: Each state change is recorded as an immutable transition, creating a clear lineage of state evolution.

4. **WebAssembly Integration**: The state system leverages WebAssembly's component model to pass state in and out of the sandbox in a type-safe manner.

## State Flow in Function Calls

```
Current State + Message Parameters → WebAssembly Handler → New State + Response
```

This pattern appears throughout the Theater system:

1. The `ActorExecutor` retrieves the current state before each function call
2. The state is passed as the first parameter to the WebAssembly function
3. The WebAssembly function returns both a new state and its response
4. The `ActorExecutor` updates the stored state with the returned value

## Implementation Details

### State Passing in WebAssembly

The WebAssembly interface for handler functions follows this pattern:

```wit
handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
```

This signature shows:
- The `state` parameter is optional (can be `null` for initialization)
- The function returns a tuple containing the new state and the response data
- The result is wrapped in a `Result` type for error handling

### Type-Safe Function Calls

The `ActorInstance` implementation manages type-safe function calls through:

```rust
pub async fn call_function(
    &mut self,
    name: &str,
    state: Option<Vec<u8>>,
    params: Vec<u8>,
) -> Result<(Option<Vec<u8>>, Vec<u8>)>
```

This function:
1. Looks up the appropriate typed function handler
2. Passes the current state and parameters to the WebAssembly component
3. Receives and returns the new state and response

### State Storage

Actor state is stored in the `ActorStore` which:
- Maintains the current state as a serialized blob
- Tracks state changes through the event chain
- Provides access to the state for inspection and manipulation

## State Lifecycle

### Initialization

1. The actor starts with a `None` (null) state
2. The initialization function is called with this null state
3. The function returns the initial state for the actor
4. This initial state is recorded in the event chain

### Updates

1. Each message handler receives the current state
2. The handler can modify the state as needed
3. The new state is returned along with the handler's response
4. The state change is recorded in the event chain

### Retrieval

Parent actors or the runtime can retrieve an actor's state:
- Through supervisor interfaces for parent actors
- Via management commands for the runtime
- For debugging or monitoring purposes

## Benefits of Runtime-Managed State

### Supervision Capabilities

The runtime-managed state enables powerful supervision features:
- Parent actors can inspect child state at any time
- Supervisors can restart actors with preserved state
- The runtime can migrate actors between systems

### Resilience and Recovery

By managing state outside the actor:
- Actors can be restarted after crashes without losing state
- Known-good states can be restored when issues occur
- State can be persisted independently of the actor

### Transparency and Debugging

The explicit state management provides:
- Clear visibility into state changes over time
- Ability to correlate state changes with specific events
- Tools for debugging state-related issues

## Future Directions

The state management system is designed to enable future capabilities:

1. **Migration**: Actors can be moved between host systems by transferring their state and event chain
2. **State Rollback**: Actors can revert to previous states when errors occur
3. **State Validation**: Supervisors can apply validation rules to state changes
4. **Optimized Storage**: State can be stored in different backends based on size and access patterns
5. **Distributed State**: Actor state could be synchronized across multiple systems

## WebAssembly State Interface

The WebAssembly Interface Type (WIT) definition for state management:

```wit
interface actor {
    use types.{state};
    
    // Initialize actor with parameters
    init: func(state: state, params: tuple<string>) -> result<tuple<state>, string>;
}

interface message-server-client {
    use types.{json, event};
    
    // Handle asynchronous messages (no response)
    handle-send: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>>, string>;
    
    // Handle synchronous requests (with response)
    handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
}
```

## Best Practices

1. **Treat State as Immutable**: Always return a new state object rather than modifying the existing one
2. **Keep State Serializable**: Ensure all state can be properly serialized and deserialized
3. **Limit State Size**: Keep state compact to improve performance
4. **Use Appropriate Data Structures**: Structure state for efficient access patterns
5. **Separate Concerns**: Divide state logically between different actors in a supervision tree
