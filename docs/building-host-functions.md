# Building Host Functions Guide

This guide explores the principles, challenges, and best practices for implementing host functions in Theater, with particular focus on handling asynchronous operations and maintaining the actor system's integrity.

## Core Principles

### 1. State Chain Integrity
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

## Conclusion

Building host functions requires careful consideration of:
- Sequential execution constraints
- State consistency requirements
- Resource management
- Error handling

Following these patterns and guidelines helps create robust, maintainable host functions that work well within Theater's actor system.