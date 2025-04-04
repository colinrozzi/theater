# Interface System

Theater's interface system is built on the WebAssembly Component Model and WebAssembly Interface Types (WIT), providing a type-safe and flexible way for actors to expose and consume functionality while maintaining verifiable state transitions.

## WebAssembly Interface Types (WIT)

Theater defines its interfaces using WIT, providing a language-agnostic way to describe component interfaces. The core WIT files are located in the `wit/` directory:

### Core Interfaces

1. **actor.wit** - Basic actor interface:
```wit
interface actor {
    use types.{state};
    
    init: func(state: state, params: tuple<string>) -> result<tuple<state>, string>;
}
```

2. **message-server.wit** - Message handling interface:
```wit
interface message-server-client {
    use types.{json, event};

    handle-send: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>>, string>;
    handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
}

interface message-server-host {
    use types.{json, actor-id};

    send: func(actor-id: actor-id, msg: json) -> result<_, string>;
    request: func(actor-id: actor-id, msg: json) -> result<json, string>;
}
```

3. **http.wit** - HTTP server and client interfaces:
```wit
interface http-server {
    use types.{state};
    use http-types.{http-request, http-response};

    handle-request: func(state: state, params: tuple<http-request>) -> result<tuple<state, tuple<http-response>>, string>;
}

interface http-client {
    use types.{json};
    use http-types.{http-request, http-response};

    send-http: func(req: http-request) -> result<http-response, string>;
}
```

4. **supervisor.wit** - Parent-child supervision:
```wit
interface supervisor {
    spawn: func(manifest: string) -> result<string, string>;
    list-children: func() -> list<string>;
    stop-child: func(child-id: string) -> result<_, string>;
    restart-child: func(child-id: string) -> result<_, string>;
    get-child-state: func(child-id: string) -> result<list<u8>, string>;
    get-child-events: func(child-id: string) -> result<list<chain-event>, string>;
    // ...
}
```

5. **types.wit** - Common data types:
```wit
interface types {
    type json = list<u8>;
    type state = option<list<u8>>;
    type actor-id = string;
    // ...
}
```

## Handler System

Theater uses a handler system to connect actor interfaces with their implementations:

### Handler Types

The current implementation includes several handler types:

1. **Message Server Handler**:
   - Handles direct actor-to-actor messaging
   - Supports both request/response and one-way sends
   - Serializes messages as JSON bytes

2. **HTTP Server Handler**:
   - Exposes actor functionality via HTTP endpoints
   - Converts HTTP requests to actor messages
   - Transforms responses back to HTTP

3. **Supervisor Handler**:
   - Enables parent-child supervision
   - Provides lifecycle management functions
   - Access to child state and events

### Handler Configuration

Handlers are configured in actor manifests:

```toml
name = "my-actor"
component_path = "my_actor.wasm"

# Message server handler
[[handlers]]
type = "message-server"
config = { port = 8080 }
interface = "ntwk:theater/message-server-client"

# HTTP server handler
[[handlers]]
type = "http-server"
config = { port = 8081 }

# Supervisor handler
[[handlers]]
type = "supervisor"
config = {}
```

## Message Flow

### Actor-to-Actor Messaging

1. **Send Message** (one-way):
   - Sender actor calls `message-server-host::send`
   - Message is routed through TheaterRuntime
   - Recipient actor's `handle-send` is called
   - State is updated and recorded in hash chain
   - No response is returned to sender

2. **Request Message** (request/response):
   - Sender actor calls `message-server-host::request`
   - Message is routed through TheaterRuntime
   - Recipient actor's `handle-request` is called
   - State is updated and recorded in hash chain
   - Response is returned to sender

### HTTP Integration

1. **Incoming HTTP Request**:
   - HTTP request arrives at server
   - Request is converted to `http-request` struct
   - Actor's `handle-request` function is called
   - Response is converted back to HTTP and returned

2. **Outgoing HTTP Request**:
   - Actor calls `http-client::send-http`
   - Request is made to external service
   - Response is returned to actor
   - Interaction is recorded in hash chain

## Interface Implementation

Actors implement interfaces through WebAssembly components:

### Required Component Structure

A Theater actor component must:
1. Implement required interfaces (based on handlers)
2. Export interface functions with correct signatures
3. Handle state consistently
4. Process messages according to interface specifications

### Example Actor Implementation

```rust
use theater_sdk::{actor, message_server};

struct CounterActor;

#[actor::export]
impl actor::Actor for CounterActor {
    fn init(state: Option<Vec<u8>>, params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        // Initialize with either existing state or new state
        let state = state.unwrap_or_else(|| {
            let initial_state = serde_json::json!({ "count": 0 });
            serde_json::to_vec(&initial_state).unwrap()
        });
        
        Ok((Some(state),))
    }
}

#[message_server::export]
impl message_server::MessageServerClient for CounterActor {
    fn handle_send(
        state: Option<Vec<u8>>,
        params: (Vec<u8>,)
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Process one-way message
        // ...
        Ok((new_state,))
    }
    
    fn handle_request(
        state: Option<Vec<u8>>,
        params: (Vec<u8>,)
    ) -> Result<(Option<Vec<u8>>, (Vec<u8>,)), String> {
        // Process request/response message
        // ...
        Ok((new_state, (response,)))
    }
}
```

## Working with State

The interface system consistently handles state:

1. **State Representation**:
   - State is represented as `Option<Vec<u8>>` (optional bytes)
   - Typically contains serialized JSON or other format
   - State is passed to and from interface functions

2. **State Updates**:
   - Functions return new state
   - Changes are recorded in hash chain
   - State is available for inspection and verification

3. **State Access**:
   - Current state is provided to interface functions
   - Functions can modify state by returning new version
   - Parent actors can access child state via supervision

## Actor Manifest

The manifest connects interfaces to implementations:

```toml
name = "counter-actor"
component_path = "counter.wasm"

# Interfaces implemented by this actor
[interface]
implements = [
    "ntwk:theater/actor",
    "ntwk:theater/message-server-client",
    "ntwk:theater/http-server"
]

# Interfaces required by this actor
requires = [
    "ntwk:theater/message-server-host"
]

# Message server handler
[[handlers]]
type = "message-server"
config = {}

# HTTP server handler
[[handlers]]
type = "http-server"
config = { port = 8080 }
```

## Interface Composition

Theater's interface system is designed for composition, allowing actors to:

1. **Implement Multiple Interfaces**:
   - Core actor functionality
   - Message handling
   - HTTP serving
   - Custom functionality

2. **Depend on Host Interfaces**:
   - Message sending
   - HTTP client
   - Supervision
   - File system access

3. **Combine Interface Types**:
   - One interface can extend another
   - Interfaces can share common types
   - Versioning through interface namespaces

Each interface maintains state chain integrity while providing a specific capability.

## Message Structure

While the interface system is flexible, messages typically follow a standard structure:

```json
{
  "type": "request_type",
  "action": "specific_operation",
  "payload": {
    "param1": "value1",
    "param2": 42
  },
  "metadata": {
    "timestamp": "2025-02-26T12:34:56Z",
    "request_id": "req-123456"
  }
}
```

Responses typically include:

```json
{
  "type": "response",
  "status": "success",
  "payload": {
    "result": "value"
  },
  "metadata": {
    "timestamp": "2025-02-26T12:34:57Z",
    "request_id": "req-123456"
  }
}
```

## Debugging Interfaces

Theater provides several mechanisms for debugging interfaces:

1. **Tracing**:
   - All interface calls are logged
   - State transitions are recorded
   - Message flow can be traced end-to-end

2. **Interface Inspection**:
   - WIT interfaces can be introspected
   - Available functions can be listed
   - Type checking for message formats

3. **State Verification**:
   - Hash chain can be verified at any point
   - State history can be examined
   - State transitions can be replayed

## Custom Interface Development

Creating new interfaces requires:

1. **WIT Definition**:
   - Define interface functions and types
   - Document expected behavior
   - Specify state handling patterns

2. **Handler Implementation**:
   - Create handler in Theater runtime
   - Connect WIT interface to actor
   - Handle message routing correctly

3. **Actor Implementation**:
   - Implement interface functions
   - Handle state properly
   - Process messages according to spec

## Best Practices

1. **Interface Design**
   - Keep interfaces focused on single responsibility
   - Use clear, descriptive function names
   - Document expected behavior
   - Provide meaningful error messages
   - Consider versioning strategy

2. **Message Design**
   - Use consistent type field for categorization
   - Include action field for specific operations
   - Structure payloads logically
   - Add metadata for debugging
   - Handle errors consistently

3. **State Management**
   - Keep state serializable
   - Handle state transitions atomically
   - Validate state after changes
   - Consider state size impacts
   - Test state rollback scenarios

4. **Security Considerations**
   - Validate all input messages
   - Sanitize data crossing interface boundaries
   - Control access to sensitive interfaces
   - Verify state integrity frequently
   - Test for message injection risks
