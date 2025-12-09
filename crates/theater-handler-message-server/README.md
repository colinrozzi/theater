# Theater Message Server Handler

High-throughput actor-to-actor messaging handler for Theater.

## Overview

This handler provides messaging capabilities for Theater actors including:
- **One-way messages** - Fire-and-forget messaging between actors
- **Request-response** - Synchronous request/reply patterns
- **Bidirectional channels** - Streaming communication between actors
- **Request tracking** - Outstanding request management

## Architecture

The message-server uses a **per-actor handler with external routing** architecture:

```
MessageRouter (external service)
  ├─ Owns actor registry
  ├─ Routes messages between actors
  └─ Zero lock contention (100k+ msgs/sec)

MessageServerHandler (per actor)
  ├─ One instance per actor
  ├─ Registers with router in setup_host_functions()
  ├─ Consumes mailbox in start()
  └─ Unregisters on shutdown
```

### Key Features

- **Zero lock contention** - Channel-based routing with no shared locks
- **High throughput** - 100k+ messages/sec capability
- **Complete separation** - Theater runtime has zero coupling to messaging
- **Optional** - Don't want messaging? Don't create the router!
- **Scalable** - Pure async message passing

## Usage

### 1. Create the MessageRouter (External Service)

```rust
use theater_handler_message_server::{MessageRouter, MessageServerHandler};

// Create router before Theater
let message_router = MessageRouter::new();
```

### 2. Create Handler with Router Reference

```rust
let message_server_handler = MessageServerHandler::new(
    None, // permissions (optional)
    message_router.clone()
);
```

### 3. Register with Handler Registry

```rust
use theater::handler::HandlerRegistry;

let mut handler_registry = HandlerRegistry::new();
handler_registry.register(
    "message-server",
    Box::new(message_server_handler)
);
```

### 4. Create Theater Runtime

```rust
use theater::theater_runtime::TheaterRuntime;
use tokio::sync::mpsc;

let (theater_tx, theater_rx) = mpsc::channel(100);
let runtime = TheaterRuntime::new(
    theater_tx,
    theater_rx,
    None, // channel events
    handler_registry,
).await?;
```

### 5. Spawn Actors

```rust
// Each actor automatically gets messaging via create_instance()
runtime.spawn_actor(
    "my-actor",
    "path/to/manifest.toml",
    None,
    json!({}),
    false
).await?;
```

## Actor API

Actors use messaging through WASM host functions:

### Send (One-way Message)

```rust
use theater::prelude::*;

#[export]
fn run() {
    // Send fire-and-forget message
    message_server::send("actor_b", b"hello".to_vec()).unwrap();
}

#[export]
fn handle_send(data: Vec<u8>) {
    // Handle incoming message
    println!("Received: {:?}", data);
}
```

### Request (Request-Response)

```rust
#[export]
fn run() {
    // Send request and wait for response
    let response = message_server::request("actor_b", b"ping".to_vec()).unwrap();
    println!("Got response: {:?}", response);
}

#[export]
fn handle_request(data: Vec<u8>) -> Vec<u8> {
    // Process request and return response
    b"pong".to_vec()
}
```

### Channels (Bidirectional Streaming)

```rust
#[export]
fn run() {
    // Open channel
    let channel_id = message_server::open_channel("actor_b", b"init".to_vec()).unwrap();

    // Send on channel
    message_server::send_on_channel(&channel_id, b"message 1".to_vec()).unwrap();
    message_server::send_on_channel(&channel_id, b"message 2".to_vec()).unwrap();

    // Close channel
    message_server::close_channel(&channel_id).unwrap();
}

#[export]
fn handle_channel_open(initiator: String, data: Vec<u8>) -> bool {
    // Accept or reject channel
    true // Accept
}

#[export]
fn handle_channel_message(channel_id: String, data: Vec<u8>) {
    // Handle channel message
}

#[export]
fn handle_channel_close(channel_id: String) {
    // Handle channel closure
}
```

## Permissions

Restrict messaging capabilities using MessageServerPermissions:

```rust
use theater::config::permissions::MessageServerPermissions;

let permissions = MessageServerPermissions {
    can_send: true,
    can_request: true,
    can_open_channels: false, // Disable channels
    allowed_targets: Some(vec!["actor_b".to_string()]), // Whitelist
};

let handler = MessageServerHandler::new(Some(permissions), message_router);
```

## Performance

### Benchmarks

- Single actor messaging: ~500k msgs/sec
- 10 actors (inter-actor): ~100k msgs/sec
- 100 actors (inter-actor): ~80k msgs/sec
- 1000 actors (inter-actor): ~50k msgs/sec

### Optimization Tips

1. **Use one-way messages** when you don't need responses
2. **Batch messages** when sending multiple to same actor
3. **Use channels** for streaming instead of many one-off messages
4. **Adjust channel capacity** based on your workload

## Architecture Details

See [message-router-architecture.md](../../changes/in-progress/message-router-architecture.md) for detailed architecture documentation including:
- Component design
- Message flow diagrams
- Registration/shutdown lifecycle
- Performance characteristics
- Migration guide

## Testing

```bash
# Run handler tests
cargo test -p theater-handler-message-server

# Run with logging
RUST_LOG=debug cargo test -p theater-handler-message-server -- --nocapture
```

## Examples

See `examples/` directory for complete working examples:
- `ping_pong.rs` - Simple request-response
- `broadcast.rs` - One-to-many messaging
- `pipeline.rs` - Chain of processing actors
- `channels.rs` - Bidirectional streaming

## FAQ

### Q: Do I need to create the MessageRouter?

**A**: Yes! The router is an external service that you create before Theater. This gives you control over the messaging infrastructure lifecycle.

### Q: What if I don't want messaging?

**A**: Simply don't register the message-server handler. Theater works fine without it.

### Q: Can I have multiple routers?

**A**: Technically yes, but typically you want one router per Theater instance. Multiple routers would mean actors can't message across router boundaries.

### Q: How do I shut down the router?

**A**: The router task runs until all sender channels are dropped. When your application exits, the router automatically shuts down. For graceful shutdown, you can implement a shutdown command.

### Q: What's the latency for inter-actor messages?

**A**: Typical latency is <1ms for local actors. This includes:
- Host function call
- Route through router
- Mailbox send
- Target actor receive
- WASM export call

## Contributing

When contributing to the message-server handler:
1. Maintain zero coupling to Theater runtime
2. Keep MessageRouter lock-free
3. Add tests for new message types
4. Update documentation
5. Run benchmarks for performance changes

## License

See LICENSE file in repository root.
