# Theater Handler System Guide

The Theater handler system provides a flexible way for actors to interact with the outside world. Each handler type provides specific capabilities while maintaining the core principles of state tracking and verification.

## Available Handlers

Theater currently supports these handler types:

1. **WebSocket Server**
   - Real-time bidirectional communication
   - Connection management
   - Multiple message types

2. **HTTP Server**
   - REST API endpoints
   - Static file serving
   - Request/response handling

3. **Message Server**
   - Simple message passing
   - JSON message format
   - HTTP transport

4. **HTTP Client**
   - External API calls
   - Request configuration
   - Response handling

5. **FileSystem**
   - File reading/writing
   - Directory management
   - Asset serving

6. **Runtime**
   - Core actor operations
   - Logging
   - Actor spawning
   - Chain access

## Handler Configuration

Handlers are configured in the actor's manifest file:

```toml
[[handlers]]
type = "websocket-server"
config = { port = 8080 }

[[handlers]]
type = "http-server"
config = { port = 8081 }

[[handlers]]
type = "filesystem"
config = { path = "assets" }
```

## WebSocket Server Handler

The WebSocket handler provides real-time bidirectional communication.

### Features
- Connection management per client
- Binary and text message support
- Ping/pong handling
- Connection lifecycle events

### Implementation Example

```rust
use bindings::exports::ntwk::theater::websocket_server::Guest as WebSocketGuest;
use bindings::ntwk::theater::websocket_server::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ChatState {
    messages: Vec<ChatMessage>,
    users: HashMap<String, UserStatus>,
}

impl WebSocketGuest for Component {
    fn handle_message(msg: WebSocketMessage, state: Json) -> (Json, WebSocketResponse) {
        let mut chat_state: ChatState = serde_json::from_slice(&state).unwrap();
        
        match msg.ty {
            MessageType::Connect => {
                // Handle new connection...
                WebSocketResponse {
                    messages: vec![WebSocketMessage {
                        ty: MessageType::Text,
                        text: Some("Welcome!".to_string()),
                        data: None,
                    }]
                }
            },
            MessageType::Text => {
                if let Some(text) = msg.text {
                    // Process chat message...
                }
                // Broadcast to all clients...
                WebSocketResponse { messages: broadcast_messages }
            },
            MessageType::Close => {
                // Handle disconnection...
                WebSocketResponse { messages: vec![] }
            },
            _ => WebSocketResponse { messages: vec![] }
        }
    }
}
```

## HTTP Server Handler

The HTTP handler enables REST APIs and static file serving.

### Features
- Route handling
- Query parameters
- Request body parsing
- Static file serving
- Custom headers

### Implementation Example

```rust
use bindings::exports::ntwk::theater::http_server::Guest as HttpGuest;
use bindings::ntwk::theater::http_server::*;

impl HttpGuest for Component {
    fn handle_request(req: HttpRequest, state: Json) -> (HttpResponse, Json) {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/api/users") => {
                let users = get_users_from_state(&state);
                (HttpResponse {
                    status: 200,
                    headers: vec![
                        ("Content-Type".to_string(), "application/json".to_string())
                    ],
                    body: Some(serde_json::to_vec(&users).unwrap()),
                }, state)
            },
            ("POST", "/api/users") => {
                if let Some(body) = req.body {
                    let new_user: User = serde_json::from_slice(&body)?;
                    let new_state = add_user_to_state(new_user, state);
                    // ...
                }
                // ...
            },
            // Static file serving
            ("GET", path) if path.starts_with("/static/") => {
                serve_static_file(path, state)
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

## Message Server Handler

The message server provides simple message passing between actors.

### Features
- JSON message format
- HTTP transport
- State management
- Error handling

### Implementation Example

```rust
use bindings::exports::ntwk::theater::message_server::Guest as MessageGuest;
use bindings::ntwk::theater::message_server::*;

impl MessageGuest for Component {
    fn handle(msg: Message, state: Json) -> (Json, Message) {
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        
        match msg.message_type.as_str() {
            "update" => {
                // Process update message...
                let response = Message {
                    message_type: "update_complete".to_string(),
                    data: serde_json::to_vec(&result).unwrap(),
                };
                (serde_json::to_vec(&current_state).unwrap(), response)
            },
            _ => (state, Message::error("Unknown message type"))
        }
    }
}
```

## Handler Best Practices

1. **State Management**
   - Keep handler state updates atomic
   - Validate state after updates
   - Handle serialization errors
   - Include state metadata

2. **Error Handling**
   - Use appropriate status codes
   - Return error messages
   - Log errors with context
   - Maintain state consistency

3. **WebSocket Handling**
   - Track connections properly
   - Handle disconnects gracefully
   - Implement heartbeat/ping
   - Manage broadcast efficiently

4. **HTTP Patterns**
   - Use REST conventions
   - Validate request data
   - Set proper headers
   - Handle all methods

5. **Message Patterns**
   - Validate message format
   - Include message context
   - Handle all message types
   - Provide clear responses

## Security Considerations

1. **Input Validation**
   - Validate all input data
   - Sanitize file paths
   - Check message sizes
   - Validate JSON structure

2. **Resource Management**
   - Limit concurrent connections
   - Handle timeouts
   - Manage memory usage
   - Clean up resources

3. **Error Exposure**
   - Limit error details
   - Log sensitive errors
   - Use appropriate codes
   - Sanitize responses

## Performance Tips

1. **WebSocket**
   - Batch messages when possible
   - Use binary for large data
   - Implement backpressure
   - Monitor connection count

2. **HTTP Server**
   - Cache static files
   - Use appropriate status codes
   - Compress responses
   - Implement timeouts

3. **Message Server**
   - Batch related changes
   - Monitor message size
   - Handle backpressure
   - Track message patterns

## Debugging Handlers

1. Use the runtime log function
2. Monitor state changes
3. Track message patterns
4. Check chain entries
5. Validate responses