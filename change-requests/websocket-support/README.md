# WebSocket Support for Theater Actors

## Motivation
Real-time bidirectional communication is crucial for many modern web applications. While HTTP handlers currently support request/response patterns, WebSocket support would enable:
- True real-time updates
- Reduced server load compared to polling
- More natural implementation of chat/messaging systems
- Lower latency for time-sensitive applications

## Technical Proposal

Add WebSocket support through:

1. New handler type in actor manifests:
```toml
[[handlers]]
type = "WebSocket"
config = { port = 8080, path = "/ws" }
```

2. New WIT interface for WebSocket operations:
```wit
interface websocket {
    record connection {
        id: string,
        // Additional metadata
    }
    
    send-message: func(connection: connection, message: string)
    broadcast: func(message: string)
    close: func(connection: connection)
}
```

3. Connection lifecycle events that actors can handle:
- on_connect
- on_message
- on_disconnect

## Implementation Considerations

- Need to manage WebSocket connection state
- Consider how to handle connection pooling
- Security considerations for connection validation
- How to integrate with existing actor state management
- Memory management for long-lived connections

## Alternative Approaches

Until WebSocket support is implemented, applications can use:
1. Long polling
2. Server-Sent Events (SSE)
3. Regular polling with optimized intervals