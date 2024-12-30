# Events in Theater

## Event Structure

Theater uses a strongly-typed event system defined in `wasm.rs`:

```rust
pub struct Event {
    pub type_: String,
    pub data: Value,
}
```

Events are wrapped in chain entries:

```rust
pub struct ChainEntry {
    pub event: Event,
    pub parent: Option<String>,
}
```

## Core Event Types

### State Events
```json
{
    "type_": "state",
    "data": {
        // The new state value
    }
}
```

### Actor Messages
```json
{
    "type_": "actor-message",
    "data": {
        "address": "target-actor-address",
        "message": {
            // Message content
        }
    }
}
```

### HTTP Requests
```json
{
    "type_": "http_request",
    "data": {
        "method": "GET",
        "uri": "/path",
        "headers": [
            ["header-name", "header-value"]
        ],
        "body": "base64-encoded-body"
    }
}
```

### Initialization
```json
{
    "type_": "init",
    "data": {
        // Initial state
    }
}
```

## Event Processing

1. **Event Creation**
   - Events are created by handlers or actors
   - Each event must have a type and data
   - Data is always a valid JSON value

2. **Chain Recording**
   - Events are wrapped in ChainEntry structures
   - Parent hash links to previous event
   - Full history is maintained

3. **Event Handling**
   ```rust
   async fn handle_event(
       state: Value, 
       event: Event
   ) -> Result<(Value, Event)>
   ```

## Special Events

### NoOp Event
```rust
impl Event {
    pub fn noop() -> Self {
        Event {
            type_: "noop".to_string(),
            data: Value::Null,
        }
    }
}
```

### Error Events
```json
{
    "type_": "error",
    "data": {
        "error": "error description",
        "context": "error context"
    }
}
```

## Event Patterns

### Request/Response
1. Create request event with response channel
2. Send to actor process
3. Await response on channel
4. Handle response or timeout

### State Updates
1. Handle event arrives
2. Process state change
3. Record new state event
4. Return updated state

### Message Forwarding
1. Receive message event
2. Transform if needed
3. Forward to target actor
4. Record in chain

## Best Practices

1. **Event Design**
   - Use clear, consistent type names
   - Include necessary context in data
   - Keep events focused and small
   - Handle all error cases

2. **Chain Management**
   - Monitor chain growth
   - Implement pruning strategies
   - Verify chain integrity
   - Handle parent references

3. **Error Handling**
   - Clear error events
   - Proper error context
   - Response channel cleanup
   - Timeout handling

4. **Testing**
   - Test all event types
   - Verify chain recording
   - Check error cases
   - Test timeouts