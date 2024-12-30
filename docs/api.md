# API Documentation

## HTTP API

### HTTP Server Interface

The HTTP server accepts requests on the configured port and transforms them into actor events.

#### Request Format
All incoming HTTP requests are converted to this format:
```rust
struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

#### Response Format
Actor responses are converted to this format:
```rust
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

### Message Server Interface

The message server handles actor-to-actor communication.

#### Message Format
```rust
struct ActorMessage {
    event: Event {
        type_: String,
        data: Value,
    },
    response_channel: Option<mpsc::Sender<Event>>,
}
```

### Chain Interface

#### ChainRequestType
```rust
pub enum ChainRequestType {
    GetHead,                    // Get latest chain entry
    GetChainEntry(String),      // Get specific entry by hash
    GetChain,                   // Get complete chain
    AddEvent { event: Event },  // Add new event
}
```

#### ChainResponse
```rust
pub enum ChainResponse {
    Head(Option<String>),
    ChainEntry(Option<ChainEntry>),
    FullChain(Vec<(String, ChainEntry)>),
}
```

#### Chain Entry Structure
```rust
pub struct ChainEntry {
    pub event: Event,
    pub parent: Option<String>,
}
```

## WebAssembly Component Interface

### Required Exports

Components must export these functions:

```rust
fn init() -> Result<Value>;
fn handle_event(state: Value, event: Event) -> Result<(Value, Event)>;
fn verify_state(state: &Value) -> bool;
```

### Host Functions

Available to components:

```rust
fn log(msg: String);
fn send(address: String, msg: Vec<u8>);
fn http_send(address: String, msg: Vec<u8>) -> Vec<u8>;
```

## Usage Examples

### HTTP Request
```bash
curl -X POST http://localhost:8080/ \
     -H "Content-Type: application/json" \
     -d '{
           "type": "user_action",
           "data": {
             "action": "update",
             "value": 42
           }
         }'
```

### Actor Message
```rust
let msg = ActorMessage {
    event: Event {
        type_: "state_update".to_string(),
        data: json!({
            "field": "value",
            "new_value": 42
        }),
    },
    response_channel: None,
};
```

### Chain Query
```rust
// Get chain head
let request = ChainRequest {
    request_type: ChainRequestType::GetHead,
    response_tx: tx,
};

// Get full chain
let request = ChainRequest {
    request_type: ChainRequestType::GetChain,
    response_tx: tx,
};
```

## Error Handling

### HTTP Status Codes
- 200: Success
- 400: Invalid request format
- 404: Resource not found
- 500: Internal server error

### Error Responses
```json
{
    "error": {
        "type": "error_type",
        "message": "Error description"
    }
}
```

## Security Notes

1. **Local Server**
   - HTTP server binds to localhost by default
   - External access must be explicitly configured

2. **Chain Integrity**
   - All state changes are recorded
   - Chain entries are immutable
   - Parent hashes ensure chain integrity

3. **Component Isolation**
   - WebAssembly provides memory isolation
   - Capability-based security model
   - Limited host function access

## Best Practices

1. **State Management**
   - Keep states small and focused
   - Validate all state transitions
   - Use meaningful event types
   - Handle all error cases

2. **Chain Management**
   - Monitor chain growth
   - Implement chain pruning if needed
   - Verify chain integrity regularly

3. **Message Design**
   - Use clear event types
   - Include necessary context
   - Handle response channels appropriately
   - Implement timeouts for responses

4. **Error Handling**
   - Log all errors
   - Return meaningful error messages
   - Handle all response channel cases
   - Implement proper cleanup

## Rate Limiting and Performance

1. **Channel Capacity**
   - Message channels have limited capacity
   - Implement backpressure handling
   - Monitor channel utilization

2. **Chain Growth**
   - Monitor chain size
   - Implement retention policies
   - Consider state snapshots

3. **Resource Usage**
   - Monitor memory usage
   - Track WebAssembly instance lifecycle
   - Implement proper cleanup