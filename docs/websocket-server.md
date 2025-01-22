# WebSocket Server Guide

This guide explains how to use WebSocket capabilities in Theater actors. WebSockets enable bidirectional communication between clients and your actor, making them perfect for real-time applications.

## Basic Concepts

The WebSocket server implementation in Theater allows actors to:
- Handle incoming WebSocket connections
- Receive messages from clients
- Send messages back to specific clients
- Maintain persistent connections

## Actor Implementation

To implement WebSocket functionality in your actor, you need to:

1. Implement the `handle-message` function from the `ntwk:theater/websocket-server` interface
2. Handle different message types (text, binary, connect, close)
3. Return responses when needed

### Example Implementation

```rust
use serde::{Deserialize, Serialize};
use bindings::exports::ntwk::theater::websocket_server::Guest;
use bindings::ntwk::theater::types::Json;

struct Component;

impl Guest for Component {
    fn handle_message(msg: WebSocketMessage, state: Json) -> (Json, WebSocketResponse) {
        // Parse current state
        let mut current_state: State = serde_json::from_slice(&state).unwrap();
        
        // Handle different message types
        match msg.message_type.as_str() {
            "connect" => {
                // Handle new connection
                let response = WebSocketResponse {
                    messages: vec![WebSocketMessage {
                        message_type: "text".to_string(),
                        text: Some("Welcome!".to_string()),
                        data: None,
                    }],
                };
                (state, response)
            },
            "text" => {
                // Handle text message
                if let Some(text) = msg.text {
                    // Process text message...
                    let response = WebSocketMessage {
                        message_type: "text".to_string(),
                        text: Some("Received your message!".to_string()),
                        data: None,
                    };
                    (state, WebSocketResponse { messages: vec![response] })
                } else {
                    (state, WebSocketResponse { messages: vec![] })
                }
            },
            "close" => {
                // Handle connection close
                (state, WebSocketResponse { messages: vec![] })
            },
            _ => (state, WebSocketResponse { messages: vec![] }),
        }
    }
}
```

## Message Types

The WebSocket interface supports several message types:

- `text`: Text messages (JSON or plain text)
- `binary`: Binary data
- `connect`: Sent when a new client connects
- `close`: Sent when a client disconnects
- `ping`/`pong`: Connection keep-alive messages

## Configuration

To enable WebSocket functionality in your actor, add the handler to your manifest:

```toml
[[handlers]]
type = "websocket-server"
config = { port = 8080 }
```

## Best Practices

1. **Handle Connection Events**
   - Always handle the `connect` message type
   - Send a welcome message to new connections
   - Clean up resources on `close` messages

2. **Message Processing**
   - Keep message processing quick and non-blocking
   - Handle all message types appropriately
   - Validate incoming messages

3. **Error Handling**
   - Handle malformed messages gracefully
   - Provide meaningful error responses
   - Log errors for debugging

## Example: Chat Room Actor

Here's a complete example of a chat room actor:

```rust
use serde::{Deserialize, Serialize};
use bindings::exports::ntwk::theater::websocket_server::Guest;
use bindings::ntwk::theater::types::Json;

#[derive(Serialize, Deserialize)]
struct ChatState {
    connected_users: Vec<String>,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    user: String,
    content: String,
    timestamp: String,
}

struct Component;

impl Guest for Component {
    fn handle_message(msg: WebSocketMessage, state: Json) -> (Json, WebSocketResponse) {
        let mut chat_state: ChatState = serde_json::from_slice(&state).unwrap();
        
        match msg.message_type.as_str() {
            "connect" => {
                // Send chat history to new user
                let history = serde_json::to_string(&chat_state.messages).unwrap();
                let response = WebSocketResponse {
                    messages: vec![WebSocketMessage {
                        message_type: "text".to_string(),
                        text: Some(history),
                        data: None,
                    }],
                };
                (serde_json::to_vec(&chat_state).unwrap(), response)
            },
            "text" => {
                if let Some(text) = msg.text {
                    // Broadcast message to all clients
                    let new_message = ChatMessage {
                        user: "anonymous".to_string(),
                        content: text,
                        timestamp: chrono::Utc::now().to_string(),
                    };
                    
                    chat_state.messages.push(new_message);
                    
                    let broadcast = serde_json::to_string(&chat_state.messages).unwrap();
                    let response = WebSocketResponse {
                        messages: vec![WebSocketMessage {
                            message_type: "text".to_string(),
                            text: Some(broadcast),
                            data: None,
                        }],
                    };
                    (serde_json::to_vec(&chat_state).unwrap(), response)
                } else {
                    (state, WebSocketResponse { messages: vec![] })
                }
            },
            _ => (state, WebSocketResponse { messages: vec![] }),
        }
    }
}
```

## Testing WebSocket Actors

You can test your WebSocket actor using tools like `websocat` or browser-based WebSocket clients:

```bash
# Using websocat
websocat ws://localhost:8080/ws
```

Or using JavaScript in the browser:

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');
ws.onmessage = (event) => {
    console.log('Received:', event.data);
};
ws.send('Hello!');
```