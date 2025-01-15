# Interface System

Theater's interface system enables actors to expose and consume functionality while maintaining verifiable state transitions and simple JSON messaging.

## Core Interface

Every actor implements a core initialization interface:

```rust
fn init() -> String
```

This creates the initial state for the actor. Additional functionality like message handling can be added through optional interfaces.

## Message Handler Interface

Actors can optionally implement the message-server-client interface to handle messages:

```rust
fn handle(event: Event, state: &str) -> String
```

This interface:
- Takes an event and current state
- Returns new state
- Is tracked in the hash chain
- Enables actor-to-actor communication

## Interface Types

### Actor-to-Actor
```toml
[interface]
implements = [
    "ntwk:simple-actor/actor",
    "ntwk:theater/message-server-client"
]
requires = []

[[handlers]]
type = "message-server"
config = { port = 8080 }
```

Note: The message-server interface is optional. Only implement it if your actor needs to handle messages.

Basic message pattern:
```json
// Input Message
{
  "type": "request",
  "action": "increment",
  "payload": {
    "amount": 5
  }
}

// Response
{
  "type": "response",
  "status": "success",
  "payload": {
    "new_count": 5
  }
}
```

### HTTP Server
```toml
[[handlers]]
type = "Http-server"
config = { port = 8080 }
```

HTTP requests transform into messages:
```json
// POST /api/v1/counter
{
  "type": "http_request",
  "method": "POST",
  "path": "/api/v1/counter",
  "body": {
    "amount": 5
  }
}
```

### HTTP Client
```toml
[[handlers]]
type = "Http-client"
config = { base_url = "http://api.example.com" }
```

Outgoing HTTP becomes messages:
```json
{
  "type": "http_client_request",
  "method": "GET",
  "path": "/api/v1/data",
  "headers": {
    "Accept": "application/json"
  }
}
```

## Interface Composition

Actors can implement multiple interfaces:

```toml
[interface]
implements = [
  "ntwk:simple-actor/actor",
  "ntwk:simple-actor/http-server",
  "ntwk:simple-actor/metrics"
]
requires = [
  "ntwk:simple-actor/http-client"
]
```

Each interface:
- Maintains hash chain integrity
- Uses consistent JSON format
- Can be independently verified

## Message Routing

Theater automatically routes messages:

```
HTTP Request -> HTTP Handler -> Actor Message -> State Change
     ↑                                              ↓
Response    <-     JSON     <- Handler Result <- Hash Chain
```

All transitions are:
- Recorded in hash chain
- Verifiable end-to-end
- Consistently formatted

## Interface Extensions

Custom interfaces use the same pattern:

```rust
// Define interface
pub trait CustomInterface {
    fn handle_custom(&self, message: &str) -> String;
}

// Implement for actor
impl CustomInterface for MyActor {
    fn handle_custom(&self, message: &str) -> String {
        // Handle message, update state, return response
    }
}
```

Registration in manifest:
```toml
[interface]
implements = ["my:custom/interface"]
```

## Best Practices

1. **Interface Design**
   - Keep interfaces focused
   - Use clear message types
   - Consider error cases
   - Document behavior

2. **Message Structure**
   - Consistent type field
   - Clear action names
   - Structured payloads
   - Error responses

3. **HTTP Integration**
   - RESTful endpoints
   - Clear status codes
   - Structured errors
   - Request validation

4. **Testing**
   - Test each interface
   - Verify state changes
   - Check error handling
   - Validate hash chain

## Example: Multi-Interface Actor

```rust
// Actor implementing multiple interfaces
struct MultiActor {
    state: String
}

impl Actor for MultiActor {
    fn init(&self) -> String {
        // Initialize actor state
        "{}".to_string()
    }
}

impl MessageServerClient for MultiActor {
    fn handle(&self, event: Event, state: &str) -> String {
        // Handle incoming messages
        // Return new state
    }
}

impl HttpServer for MultiActor {
    fn handle_request(&self, req: Request) -> Response {
        // Handle HTTP requests
    }
}

impl Metrics for MultiActor {
    fn collect_metrics(&self) -> String {
        // Expose metrics
    }
}
```

Configuration:
```toml
name = "multi-actor"
component_path = "multi_actor.wasm"

[interface]
implements = [
  "ntwk:simple-actor/actor",
  "ntwk:theater/message-server-client",
  "ntwk:simple-actor/http-server",
  "ntwk:simple-actor/metrics"
]

[[handlers]]
type = "Http-server"
config = { port = 8080 }

[[handlers]]
type = "Metrics"
config = { path = "/metrics" }
```

## Debugging Interfaces

Theater provides tools for interface debugging:

1. Message Tracing:
```json
{
  "type": "trace_request",
  "interface": "http-server",
  "timestamp": "2025-01-14T10:00:00Z",
  "messages": [
    {
      "direction": "in",
      "message": { /* ... */ }
    },
    {
      "direction": "out",
      "message": { /* ... */ }
    }
  ]
}
```

2. Interface Verification:
```json
{
  "type": "verify_interface",
  "interface": "ntwk:simple-actor/actor",
  "checks": [
    {
      "name": "message_format",
      "status": "passed"
    },
    {
      "name": "state_transitions",
      "status": "passed"
    }
  ]
}
```

3. State Impact Analysis:
```json
{
  "type": "state_impact",
  "interface": "http-server",
  "path": "/api/v1/counter",
  "affects": [
    "count",
    "last_updated"
  ]
}
```

## Security Considerations

1. **Interface Isolation**
   - Separate concerns
   - Validate inputs
   - Handle errors
   - Rate limit

2. **State Access**
   - Control mutations
   - Validate changes
   - Audit access
   - Version state

3. **HTTP Security**
   - Use HTTPS
   - Validate routes
   - Check methods
   - Sanitize inputs