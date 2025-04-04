# Building Host Functions Guide

This guide explores the principles, challenges, and best practices for implementing host functions in Theater, with particular focus on handling asynchronous operations and maintaining the actor system's integrity.

## Core Principles

### 1. Consistent Parameter Patterns
- WIT interfaces should use tuple-based parameter patterns
- Client functions should always receive state as their first parameter
- Parameters should be bundled in tuples for consistency

### 2. State Chain Integrity
- Every state transition must be properly recorded in the hash chain
- State updates must be atomic and consistent
- The chain must remain verifiable at all times

### 2. Non-Blocking Operation
- Host functions should avoid blocking the actor system
- Long-running operations should be structured to allow progress
- State transitions should be quick and deterministic

### 3. Sequential Guarantee Management
- WebAssembly component calls are inherently sequential
- Host functions must be designed with this limitation in mind
- Complex async operations need careful structuring

## Common Challenges

### Sequential Call Limitation
The WebAssembly component model requires that calls be sequential and return before making progress. This creates challenges for operations that are inherently concurrent or long-running, such as:
- Websocket connections
- Long-polling HTTP requests
- File watchers
- Database connections

### Solutions and Patterns

#### 1. Event Queue Pattern
Instead of blocking on handlers, implement an event queue system:
```rust
struct WebSocketHost {
    event_queue: Arc<Mutex<VecDeque<WebSocketEvent>>>,
    connections: Arc<Mutex<HashMap<ConnectionId, WebSocket>>>,
}

enum WebSocketEvent {
    NewConnection(ConnectionId, WebSocket),
    Message(ConnectionId, Message),
    Disconnection(ConnectionId),
}

impl WebSocketHost {
    fn process_events(&mut self) {
        while let Some(event) = self.event_queue.lock().unwrap().pop_front() {
            match event {
                WebSocketEvent::NewConnection(id, ws) => {
                    // Handle new connection without blocking
                    self.connections.lock().unwrap().insert(id, ws);
                    // Notify actor of new connection
                    self.notify_actor_connection(id);
                }
                // Handle other events...
            }
        }
    }
}
```

#### 2. State Machine Approach
Model long-running operations as state machines:
```rust
enum ConnectionState {
    Connecting,
    Connected(WebSocket),
    Closing,
    Closed,
}

struct Connection {
    state: ConnectionState,
    events: VecDeque<WebSocketEvent>,
    last_processed: Instant,
}
```

#### 3. Async Operation Splitting
Break long-running operations into discrete steps:
1. Operation initiation
2. Progress checking
3. Result collection

### Best Practices

1. **Event Buffering**
   - Buffer events when they can't be processed immediately
   - Implement reasonable buffer limits
   - Handle buffer overflow gracefully

2. **Resource Management**
   - Track resource usage carefully
   - Implement proper cleanup mechanisms
   - Handle resource exhaustion gracefully

3. **Error Handling**
   - Propagate errors appropriately
   - Maintain system stability during errors
   - Log errors with context for debugging

4. **State Consistency**
   - Ensure state transitions are atomic
   - Validate state after transitions
   - Handle partial failures gracefully

## Interface Design Guidelines

### 1. WIT Interface Design
- Define client-side functions with consistent state parameter patterns:
  ```wit
  handle-function: func(state: option<json>, params: tuple<param1-type, param2-type>) -> result<tuple<option<json>, return-type>, string>;
  ```
- The first parameter is always the actor's state
- The second parameter is always a tuple containing function parameters
- The result includes both the new state and function result

### 2. Host Implementation
- When implementing host-side code that calls client functions, use natural Rust syntax:
  ```rust
  actor_handle
    .call_function::<(ParamType1, ParamType2), ReturnType>(
      "interface.function-name",
      (param1, param2),
    )
    .await?;
  ```
- The type parameters to `call_function` should match the WIT interface
- The adapter layer handles wrapping parameters to match the tuple-based interface

### 3. Function Registration
- Register functions with types matching the WIT interface:
  ```rust
  actor_instance
    .register_function_no_result::<(ParamType1, ParamType2)>(
      "interface",
      "function-name",
    )
  ```

### 4. Example: Channel Functions
- For channel operations, follow the same parameter pattern:

  **WIT Interface**:
  ```wit
  // Correct pattern with tuple-based parameters
  handle-channel-message: func(state: option<json>, params: tuple<channel-id, json>) -> result<tuple<option<json>>, string>;
  handle-channel-close: func(state: option<json>, params: tuple<channel-id>) -> result<tuple<option<json>>, string>;
  ```

  **Host Implementation**:
  ```rust
  // Standard Rust syntax for calling the functions
  actor_handle
    .call_function::<(String, Vec<u8>), ()>(
      "ntwk:theater/message-server-client.handle-channel-message",
      (channel_id.to_string(), data),
    )
    .await?;
  ```

  **Function Registration**:
  ```rust
  // Register with types matching the WIT interface
  actor_instance
    .register_function_no_result::<(String, Vec<u8>)>(
      "ntwk:theater/message-server-client",
      "handle-channel-message",
    )
  ```

## Implementation Guidelines

### 1. Planning Phase
- Map out all possible states and transitions
- Identify potential blocking operations
- Plan error handling strategy
- Consider resource limitations

### 2. Implementation Phase
- Start with a clear state model
- Implement event buffering early
- Add comprehensive logging
- Build in failure handling

### 3. Testing Phase
- Test concurrent operations
- Verify state consistency
- Check resource cleanup
- Test error conditions

## WebSocket Host Example

Here's an improved approach to WebSocket hosting:

```rust
struct WebSocketHost {
    connections: Arc<Mutex<HashMap<ConnectionId, Connection>>>,
    event_queue: Arc<Mutex<VecDeque<WebSocketEvent>>>,
    config: WebSocketConfig,
}

impl WebSocketHost {
    fn process_events(&mut self) -> Result<(), HostError> {
        // Process a batch of events
        let mut events = self.event_queue.lock().unwrap();
        let batch: Vec<_> = events.drain(..min(events.len(), MAX_BATCH_SIZE)).collect();
        
        for event in batch {
            match event {
                WebSocketEvent::NewConnection(id, ws) => {
                    self.handle_new_connection(id, ws)?;
                }
                WebSocketEvent::Message(id, msg) => {
                    self.handle_message(id, msg)?;
                }
                WebSocketEvent::Disconnection(id) => {
                    self.handle_disconnection(id)?;
                }
            }
        }
        
        Ok(())
    }
    
    fn handle_new_connection(&mut self, id: ConnectionId, ws: WebSocket) -> Result<(), HostError> {
        // Add to connections without blocking
        self.connections.lock().unwrap().insert(id, Connection::new(ws));
        
        // Notify actor through chain
        self.notify_actor_connection(id)
    }
}
```

## Understanding Parameter Wrapping

The Theater runtime handles parameter conversion between Rust function calls and WebAssembly interfaces. Here's how it works:

### 1. Parameter Flow

1. **Host Call Layer**: When calling `actor_handle.call_function<P, R>(...)`, the parameters are serialized to JSON bytes:
   ```rust
   let params = serde_json::to_vec(&params)?
   ```

2. **Executor Layer**: The `execute_call` function passes state and parameters to the actor instance:
   ```rust
   let (new_state, results) = self.actor_instance.call_function(&name, state, params).await
   ```

3. **Adapter Layer**: The `TypedFunction` implementation deserializes parameters and calls the appropriately typed function:
   ```rust
   let params_deserialized: P = serde_json::from_slice(&params)?
   match self.call_func(store, state, params_deserialized).await ...
   ```

4. **WebAssembly Layer**: The parameters are passed to the WebAssembly function according to the WIT interface, with state as the first parameter and parameters as a tuple.

### 2. Return Flow

1. **WebAssembly Layer**: The function returns a result containing the new state and return value.

2. **Adapter Layer**: The result is serialized back to JSON bytes:
   ```rust
   let result_serialized = serde_json::to_vec(&result)?
   ```

3. **Executor Layer**: The new state is stored in the actor store:
   ```rust
   self.actor_instance.store.data_mut().set_state(new_state);
   ```

4. **Host Call Layer**: The result is deserialized back to the expected return type:
   ```rust
   let res = serde_json::from_slice::<R>(&result)?;
   ```

### 3. Type Mapping

The type parameters used in `call_function<P, R>` and `register_function*` functions should match the WebAssembly interface definition, but the adapter layer handles the specifics of matching the tuple structure. This lets you use natural Rust parameter patterns while maintaining a consistent WIT interface.

## Troubleshooting Common Issues

### 1. Blocking Operations
**Problem**: Operation blocks progress
**Solution**: Convert to event-based handling

### 2. Resource Leaks
**Problem**: Resources not properly cleaned up
**Solution**: Implement proper cleanup in all exit paths

### 3. State Inconsistency
**Problem**: State becomes invalid during concurrent operations
**Solution**: Use atomic operations and validate state transitions

### 4. Parameter Pattern Mismatch
**Problem**: WIT interface defines tuple parameters but implementation doesn't match
**Solution**: Ensure WIT interface uses consistent tuple pattern for parameters:
  ```wit
  // CORRECT
  handle-function: func(state: option<json>, params: tuple<type1, type2>) -> ...;
  
  // INCORRECT
  handle-function: func(state: option<json>, param1: type1, param2: type2) -> ...;
  ```
And ensure the host implementation uses matching types in function registration.

## Conclusion

Building host functions requires careful consideration of:
- Consistent parameter patterns in WIT interfaces
- Sequential execution constraints
- State consistency requirements
- Resource management
- Error handling

Following these patterns and guidelines helps create robust, maintainable host functions that work well within Theater's actor system. In particular, consistently using tuple-based parameter patterns in the WIT interface while leveraging the adapter layer to maintain natural Rust code creates a clean separation between interface definition and implementation.