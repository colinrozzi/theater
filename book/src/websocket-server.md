# WebSocket Server in Theater

WebSockets provide a bidirectional communication channel between clients and Theater actors, enabling real-time applications. This documentation explains the current implementation and usage patterns.

## WebSocket Interface

The WebSocket interface in Theater is defined in `wit/websocket.wit`:

```wit
package ntwk:theater;

interface websocket-types {
    enum message-type {
        text,
        binary,
        ping,
        pong,
        close,
        connect,
    }

    record websocket-message {
        ty: message-type,
        text: option<string>,
        data: option<list<u8>>,
    }

    record websocket-response {
        messages: list<websocket-message>,
    }
}

interface websocket-server {
    use types.{state};
    use websocket-types.{websocket-message, websocket-response};

    handle-message: func(state: state, params: tuple<websocket-message>) -> result<tuple<state, tuple<websocket-response>>, string>;
}
```

This interface defines:
- Message types (text, binary, ping, pong, connect, close)
- Message structure with optional text and binary data
- Response structure for sending messages back to clients
- Handler function for processing incoming messages

## Handler Configuration

To enable WebSocket functionality in your actor, add the handler to your manifest:

```toml
[[handlers]]
type = "websocket-server"
config = { port = 8080 }
```

Configuration options include:
- `port`: The port to listen on for WebSocket connections
- `path`: (Optional) The base path for WebSocket connections (default: "/")
- `max_connections`: (Optional) Maximum number of concurrent connections

## Implementing WebSocket Actors

### Basic Implementation

```rust
use theater_sdk::{websocket_server};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct EchoState {
    connection_count: u32,
    message_count: u32,
}

struct EchoActor;

#[websocket_server::export]
impl websocket_server::WebSocketServer for EchoActor {
    fn handle_message(
        state: Option<Vec<u8>>,
        params: (WebSocketMessage,)
    ) -> Result<(Option<Vec<u8>>, (WebSocketResponse,)), String> {
        // Extract message and state
        let message = params.0;
        let mut actor_state: EchoState = match state {
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("Failed to deserialize state: {}", e))?,
            None => EchoState { connection_count: 0, message_count: 0 },
        };
        
        // Handle message based on type
        match message.ty {
            MessageType::Connect => {
                // New client connection
                actor_state.connection_count += 1;
                
                // Create welcome message
                let response = WebSocketResponse {
                    messages: vec![WebSocketMessage {
                        ty: MessageType::Text,
                        text: Some(format!("Welcome! You are connection #{}", 
                            actor_state.connection_count)),
                        data: None,
                    }],
                };
                
                // Update state and return response
                let new_state = serde_json::to_vec(&actor_state)
                    .map_err(|e| format!("Failed to serialize state: {}", e))?;
                Ok((Some(new_state), (response,)))
            },
            
            MessageType::Text => {
                // Echo text message back to client
                actor_state.message_count += 1;
                
                let response_text = if let Some(text) = message.text {
                    format!("Echo: {}", text)
                } else {
                    "Received empty text message".to_string()
                };
                
                let response = WebSocketResponse {
                    messages: vec![WebSocketMessage {
                        ty: MessageType::Text,
                        text: Some(response_text),
                        data: None,
                    }],
                };
                
                let new_state = serde_json::to_vec(&actor_state)
                    .map_err(|e| format!("Failed to serialize state: {}", e))?;
                Ok((Some(new_state), (response,)))
            },
            
            MessageType::Binary => {
                // Echo binary message back to client
                actor_state.message_count += 1;
                
                let response = WebSocketResponse {
                    messages: vec![WebSocketMessage {
                        ty: MessageType::Binary,
                        text: None,
                        data: message.data.clone(),
                    }],
                };
                
                let new_state = serde_json::to_vec(&actor_state)
                    .map_err(|e| format!("Failed to serialize state: {}", e))?;
                Ok((Some(new_state), (response,)))
            },
            
            MessageType::Close => {
                // Client disconnected
                if actor_state.connection_count > 0 {
                    actor_state.connection_count -= 1;
                }
                
                let new_state = serde_json::to_vec(&actor_state)
                    .map_err(|e| format!("Failed to serialize state: {}", e))?;
                Ok((Some(new_state), (WebSocketResponse { messages: vec![] },)))
            },
            
            // Handle other message types
            _ => Ok((state, (WebSocketResponse { messages: vec![] },)))
        }
    }
}
```

### Message Handling

The `handle_message` function processes incoming WebSocket messages:

1. Message Types:
   - `Connect`: Sent when a new client connects
   - `Text`: Text messages (often JSON)
   - `Binary`: Binary data messages
   - `Ping`/`Pong`: Connection keep-alive messages
   - `Close`: Sent when a client disconnects

2. Response Structure:
   ```rust
   WebSocketResponse {
       messages: Vec<WebSocketMessage>
   }
   ```
   - You can send multiple messages in a single response
   - Messages can be of different types
   - Responses are directed to the client that sent the message

### State Management

WebSocket actors maintain state just like other actors:

1. **State Persistence**:
   - State is passed to `handle_message`
   - Function returns new state
   - All state transitions recorded in hash chain

2. **Connection Management**:
   - Theater tracks WebSocket connections internally
   - Connections are associated with the actor instance
   - Actors don't need to maintain connection lists

## Advanced Patterns

### Chat Room Example

```rust
use theater_sdk::{websocket_server};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct ChatState {
    messages: Vec<ChatMessage>,
    user_count: u32,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    username: String,
    content: String,
    timestamp: u64,
}

struct ChatActor;

#[websocket_server::export]
impl websocket_server::WebSocketServer for ChatActor {
    fn handle_message(
        state: Option<Vec<u8>>,
        params: (WebSocketMessage,)
    ) -> Result<(Option<Vec<u8>>, (WebSocketResponse,)), String> {
        // Initialize or extract state
        let mut chat_state = match state {
            Some(bytes) => serde_json::from_slice::<ChatState>(&bytes)
                .map_err(|e| format!("Failed to deserialize state: {}", e))?,
            None => ChatState {
                messages: Vec::new(),
                user_count: 0,
            },
        };
        
        let message = params.0;
        
        match message.ty {
            MessageType::Connect => {
                // User connected
                chat_state.user_count += 1;
                
                // Send chat history to new user
                let welcome = WebSocketMessage {
                    ty: MessageType::Text,
                    text: Some(format!("Welcome! There are {} users online.", 
                        chat_state.user_count)),
                    data: None,
                };
                
                let history = serde_json::to_string(&chat_state.messages)
                    .map_err(|e| format!("Failed to serialize history: {}", e))?;
                    
                let history_msg = WebSocketMessage {
                    ty: MessageType::Text,
                    text: Some(history),
                    data: None,
                };
                
                let response = WebSocketResponse {
                    messages: vec![welcome, history_msg],
                };
                
                let new_state = serde_json::to_vec(&chat_state)
                    .map_err(|e| format!("Failed to serialize state: {}", e))?;
                    
                Ok((Some(new_state), (response,)))
            },
            
            MessageType::Text => {
                // Process chat message
                if let Some(text) = message.text {
                    // Parse message (assuming JSON format)
                    let new_message: Result<ChatMessage, _> = serde_json::from_str(&text);
                    
                    match new_message {
                        Ok(msg) => {
                            // Add message to history
                            chat_state.messages.push(msg);
                            
                            // Broadcast to all users
                            let broadcast = serde_json::to_string(&chat_state.messages)
                                .map_err(|e| format!("Failed to serialize messages: {}", e))?;
                                
                            let broadcast_msg = WebSocketMessage {
                                ty: MessageType::Text,
                                text: Some(broadcast),
                                data: None,
                            };
                            
                            let response = WebSocketResponse {
                                messages: vec![broadcast_msg],
                            };
                            
                            let new_state = serde_json::to_vec(&chat_state)
                                .map_err(|e| format!("Failed to serialize state: {}", e))?;
                                
                            Ok((Some(new_state), (response,)))
                        },
                        Err(e) => {
                            // Send error to user
                            let error_msg = WebSocketMessage {
                                ty: MessageType::Text,
                                text: Some(format!("Error parsing message: {}", e)),
                                data: None,
                            };
                            
                            let response = WebSocketResponse {
                                messages: vec![error_msg],
                            };
                            
                            Ok((state, (response,)))
                        }
                    }
                } else {
                    // Empty message
                    Ok((state, (WebSocketResponse { messages: vec![] },)))
                }
            },
            
            MessageType::Close => {
                // User disconnected
                if chat_state.user_count > 0 {
                    chat_state.user_count -= 1;
                }
                
                let new_state = serde_json::to_vec(&chat_state)
                    .map_err(|e| format!("Failed to serialize state: {}", e))?;
                    
                Ok((Some(new_state), (WebSocketResponse { messages: vec![] },)))
            },
            
            // Handle other message types
            _ => Ok((state, (WebSocketResponse { messages: vec![] },)))
        }
    }
}
```

### Broadcasting to All Clients

Currently, WebSocket responses are sent only to the client that initiated the request. To implement broadcasting, messages must be stored in the actor state and sent to each client individually as they connect or send messages.

In a future enhancement, Theater may support explicit broadcasting capabilities.

## Client-Side Integration

To connect to a Theater WebSocket actor from a web browser:

```javascript
// Connect to WebSocket server
const socket = new WebSocket('ws://localhost:8080/');

// Handle connection open
socket.addEventListener('open', (event) => {
    console.log('Connected to Theater WebSocket');
    
    // Send a message
    const message = {
        username: 'User123',
        content: 'Hello, Theater!',
        timestamp: Date.now()
    };
    
    socket.send(JSON.stringify(message));
});

// Handle incoming messages
socket.addEventListener('message', (event) => {
    console.log('Received message:', event.data);
    
    try {
        // Parse JSON messages
        const data = JSON.parse(event.data);
        // Process data...
    } catch (e) {
        // Handle text messages
        console.log('Received text message:', event.data);
    }
});

// Handle connection close
socket.addEventListener('close', (event) => {
    console.log('Connection closed:', event.code, event.reason);
});

// Handle errors
socket.addEventListener('error', (event) => {
    console.error('WebSocket error:', event);
});
```

## Testing WebSocket Actors

You can test your WebSocket actors using:

1. **Command-line Tools**:
   ```bash
   # Using websocat
   websocat ws://localhost:8080/
   
   # Using wscat
   wscat -c ws://localhost:8080/
   ```

2. **Browser Dev Tools**:
   - Create a simple HTML page with WebSocket client code
   - Use browser console to interact with the WebSocket

3. **Postman or Insomnia**:
   - These API tools support WebSocket connections
   - Allow sending and receiving formatted messages

## Best Practices

### Performance Considerations

1. **Message Size**:
   - Keep messages small and focused
   - Consider compression for large payloads
   - Batch small messages when appropriate

2. **State Management**:
   - Limit message history size
   - Implement pagination for large datasets
   - Consider pruning old messages

3. **Connection Handling**:
   - Implement heartbeat mechanisms
   - Handle reconnection gracefully
   - Monitor connection count

### Security Best Practices

1. **Input Validation**:
   - Validate all incoming messages
   - Sanitize user input
   - Check message size and format

2. **Authentication**:
   - Implement token-based authentication
   - Validate credentials on connection
   - Enforce access control

3. **Rate Limiting**:
   - Limit message frequency
   - Protect against DoS attacks
   - Monitor abusive patterns

## Future Enhancements

Future versions of Theater's WebSocket implementation may include:

1. **Direct Broadcasting**:
   - Built-in support for sending to all clients
   - Targeted messaging to specific clients
   - Room/channel abstractions

2. **Binary Message Optimization**:
   - Improved handling of large binary payloads
   - Streaming support for binary data
   - Compression options

3. **Connection Management**:
   - More detailed connection lifecycle events
   - Connection metadata and context
   - Advanced monitoring capabilities

4. **Protocol Extensions**:
   - Support for WebSocket subprotocols
   - Custom protocol negotiation
   - Integration with other protocols
