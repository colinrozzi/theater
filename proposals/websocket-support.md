# WebSocket Support in Theater

## Overview
This proposal outlines adding WebSocket support to the Theater actor system, allowing actors to handle real-time bidirectional communication through WebSocket connections.

## Motivation
Currently, Theater supports HTTP communication, but real-time updates require polling. WebSocket support would enable:
- More efficient real-time communication
- Reduced server load compared to polling
- Better user experience for chat-like applications
- Natural fit for actor-based message passing

## Technical Approach

### 1. WebSocket Handshake
Extend the existing HTTP interface to handle WebSocket upgrade requests:

```wit
interface websocket-upgrade {
    // Represents WebSocket handshake information
    record websocket-request {
        sec-websocket-key: string,
        sec-websocket-version: string,
        // Additional headers as needed
    }

    record websocket-response {
        sec-websocket-accept: string,
        // Additional headers for the upgrade response
    }

    // Function to handle upgrade requests
    upgrade-connection: func(request: websocket-request) -> option<websocket-response>;
}
```

### 2. WebSocket Connection Management
Add new interfaces for managing WebSocket connections:

```wit
interface websocket {
    // WebSocket message types
    variant message-type {
        text(string),
        binary(list<u8>),
        ping,
        pong,
        close
    }

    // Connection events
    variant connection-event {
        connected,
        message(message-type),
        error(string),
        closed
    }

    // Connection handling
    type connection-id = u64;
    send-message: func(conn: connection-id, message: message-type) -> ();
    close-connection: func(conn: connection-id) -> ();
}
```

### 3. Actor Integration
Extend the actor interface to support WebSocket events:

```wit
interface websocket-actor {
    use websocket.{connection-event, message-type};
    
    // Handle WebSocket events
    handle-websocket: func(event: connection-event) -> option<message-type>;
}
```

## Implementation Phases

1. Basic WebSocket Support
   - Implement WebSocket handshake protocol
   - Basic message framing
   - Connection management

2. Actor System Integration
   - WebSocket event handling in actors
   - Connection lifecycle management
   - Error handling and recovery

3. Advanced Features
   - Connection pooling
   - Automatic reconnection
   - Heartbeat/ping management
   - Binary message support

## Considerations

### Security
- Need to validate WebSocket upgrade requests
- Consider origin policies
- Handle connection timeouts
- Implement rate limiting

### Performance
- Connection pooling for efficiency
- Message batching options
- Resource limits per connection

### Compatibility
- Support WebSocket protocol versions
- Handle various client implementations
- Backward compatibility with HTTP

## Examples

### HTTP to WebSocket Upgrade
```rust
impl HttpGuest for Component {
    fn handle_request(request: HttpRequest) -> HttpResponse {
        if is_websocket_upgrade(&request) {
            return handle_websocket_upgrade(request);
        }
        // Normal HTTP handling
    }
}
```

### WebSocket Actor
```rust
impl WebSocketGuest for Component {
    fn handle_websocket(event: ConnectionEvent) -> Option<MessageType> {
        match event {
            ConnectionEvent::Connected => {
                // Handle new connection
            }
            ConnectionEvent::Message(msg) => {
                // Handle incoming message
            }
            ConnectionEvent::Closed => {
                // Clean up connection
            }
        }
    }
}
```

## Next Steps

1. Review and refine the proposal
2. Prototype basic WebSocket handshake
3. Implement core WebSocket protocol
4. Design and implement actor integration
5. Testing and documentation
6. Performance optimization

## Open Questions

1. How should we handle connection persistence across actor restarts?
2. What's the best way to implement connection pooling?
3. Should we support WebSocket extensions?
4. How do we handle large messages efficiently?