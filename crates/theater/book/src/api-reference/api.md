# Theater API Documentation

This page provides an overview of the Theater API. For detailed reference documentation, you can check out the [auto-generated rustdoc API Reference](/theater/api/theater/index.html).

<div class="api-note">
<p><strong>Note:</strong> The rustdoc API Reference provides detailed information about all types, functions, and modules directly from the code annotations.</p>
</div>

## Core Concepts

Theater uses WebAssembly components to create isolated, deterministic actors that communicate through a message-passing interface. Each actor is a WebAssembly component that implements specific interfaces defined using the WebAssembly Interface Type (WIT) system.

## Key API Components

Here are some key components in the API:

- [ActorRuntime](/theater/api/theater/actor_runtime/struct.ActorRuntime.html) - Manages the lifecycle of an actor
- [ActorExecutor](/theater/api/theater/actor_executor/struct.ActorExecutor.html) - Executes actor code in WebAssembly
- [StateChain](/theater/api/theater/chain/struct.StateChain.html) - Maintains the verifiable chain of state changes
- [TheaterId](/theater/api/theater/id/struct.TheaterId.html) - Unique identifier for actors
- [ContentStore](/theater/api/theater/store/struct.ContentStore.html) - Content-addressable storage system

## Core Actor Interface

Every Theater actor must implement the core actor interface:

```wit
// theater:simple/actor interface
package theater:simple

interface types {
    /// JSON-encoded data
    type json = list<u8>
    
    /// Event structure for actor messages
    record event {
        event-type: string,
        parent: option<u64>,
        data: json
    }
}

interface actor {
    use types.{json, event}

    /// Initialize actor state
    init: func() -> json

    /// Handle an incoming event, returning new state
    handle: func(evt: event, state: json) -> json
}
```

### Implementation Example

Here's how to implement the core actor interface in Rust:

```rust
use bindings::exports::ntwk::theater::actor::Guest as ActorGuest;
use bindings::ntwk::theater::types::{Event, Json};
use serde::{Deserialize, Serialize};

// Define your actor's state
#[derive(Serialize, Deserialize)]
struct State {
    count: i32,
    last_updated: String,
}

struct Component;

impl ActorGuest for Component {
    // Initialize actor state
    fn init() -> Vec<u8> {
        let initial_state = State {
            count: 0,
            last_updated: chrono::Utc::now().to_string(),
        };
        
        serde_json::to_vec(&initial_state).unwrap()
    }

    // Handle incoming messages
    fn handle(evt: Event, state: Vec<u8>) -> Vec<u8> {
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        
        // Process the event
        if let Ok(message) = serde_json::from_slice(&evt.data) {
            // Update state based on message...
        }
        
        serde_json::to_vec(&current_state).unwrap()
    }
}

bindings::export!(Component with_types_in bindings);
```

## Available Host Functions

Theater provides several host functions that actors can use. For complete details, see the [host module documentation](/theater/api/theater/host/index.html).

### Runtime Interface

```wit
// theater:simple/runtime interface
interface runtime {
    /// Log a message to the host system
    log: func(msg: string)

    /// Spawn a new actor from a manifest
    spawn: func(manifest: string)

    /// Get the current event chain
    get-chain: func() -> chain
}
```

### HTTP Server Interface

```wit
// theater:simple/http-server interface
interface http-server {
    record http-request {
        method: string,
        path: string,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>
    }

    record http-response {
        status: u16,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>
    }

    handle-request: func(req: http-request, state: json) -> tuple<http-response, json>
}
```

The [HttpFramework](/theater/api/theater/host/framework/struct.HttpFramework.html) provides the implementation of this interface.

### WebSocket Server Interface

```wit
// theater:simple/websocket-server interface
interface websocket-server {
    use types.{json}

    /// Types of WebSocket messages
    enum message-type {
        text,
        binary,
        connect,
        close,
        ping,
        pong,
        other(string)
    }

    /// WebSocket message structure
    record websocket-message {
        ty: message-type,
        data: option<list<u8>>,
        text: option<string>
    }

    /// WebSocket response structure
    record websocket-response {
        messages: list<websocket-message>
    }

    /// Handle an incoming WebSocket message
    handle-message: func(msg: websocket-message, state: json) -> tuple<json, websocket-response>
}
```

## Handler Implementation Examples

### HTTP Server Handler

```rust
use bindings::exports::ntwk::theater::http_server::Guest as HttpGuest;
use bindings::ntwk::theater::types::Json;
use bindings::ntwk::theater::http_server::{HttpRequest, HttpResponse};

impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/count") => {
                let current_state: State = serde_json::from_slice(&state).unwrap();
                
                (HttpResponse {
                    status: 200,
                    headers: vec![
                        ("Content-Type".to_string(), "application/json".to_string())
                    ],
                    body: Some(serde_json::json!({
                        "count": current_state.count
                    }).to_string().into_bytes()),
                }, state)
            },
            _ => (HttpResponse {
                status: 404,
                headers: vec![],
                body: None,
            }, state)
        }
    }
}
```

For more details on HTTP handling, see the [HTTP Client documentation](/theater/api/theater/host/http_client/struct.HttpClientHost.html).

### WebSocket Server Handler

```rust
use bindings::exports::ntwk::theater::websocket_server::Guest as WebSocketGuest;
use bindings::ntwk::theater::types::Json;
use bindings::ntwk::theater::websocket_server::{
    WebSocketMessage,
    WebSocketResponse,
    MessageType
};

impl WebSocketGuest for Component {
    fn handle_message(msg: WebSocketMessage, state: Json) -> (Json, WebSocketResponse) {
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        
        let response = match msg.ty {
            MessageType::Text => {
                if let Some(text) = msg.text {
                    // Process text message...
                    WebSocketResponse {
                        messages: vec![WebSocketMessage {
                            ty: MessageType::Text,
                            text: Some("Message received".to_string()),
                            data: None,
                        }]
                    }
                } else {
                    WebSocketResponse { messages: vec![] }
                }
            },
            _ => WebSocketResponse { messages: vec![] }
        };
        
        (serde_json::to_vec(&current_state).unwrap(), response)
    }
}
```

## Actor Configuration

Actors are configured using TOML manifests. See the [ManifestConfig](/theater/api/theater/config/struct.ManifestConfig.html) for details on the configuration options.

```toml
name = "example-actor"
component_path = "target/wasm32-wasi/release/example_actor.wasm"

[interface]
implements = "theater:simple/websocket-server"
requires = []

[[handlers]]
type = "websocket-server"
config = { port = 8080 }

[logging]
level = "debug"
```

## Hash Chain Integration

Theater uses a hash chain to track state transitions. See the [StateChain](/theater/api/theater/chain/struct.StateChain.html) for more details.

## Best Practices

1. **State Management**
   - Use serde for state serialization
   - Keep state JSON-serializable
   - Include timestamps in state
   - Handle serialization errors

2. **Message Handling**
   - Validate message format
   - Handle all message types
   - Return consistent responses
   - Preserve state on errors

3. **Handler Implementation**
   - Implement appropriate interfaces
   - Handle all request types
   - Return proper responses
   - Maintain state consistency

4. **Error Handling**
   - Log errors with context
   - Return unchanged state on error
   - Validate all inputs
   - Handle all error cases

## Development Tips

1. Use the chat-room example as a reference implementation
2. Test with multiple handler types
3. Monitor the hash chain during development
4. Use logging for debugging
5. Validate state transitions