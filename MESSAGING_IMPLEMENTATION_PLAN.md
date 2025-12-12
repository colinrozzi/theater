# Messaging Features Implementation Plan

## Overview

The new MessageRouter architecture provides high-throughput (100k+ msgs/sec) message routing without lock contention. This document outlines the path forward for integrating messaging features into TheaterServer.

## Architecture Understanding

### MessageRouter (Global Service)

The MessageRouter is a single async task that owns the actor registry:

```rust
pub struct MessageRouter {
    command_tx: Sender<RouterCommand>,
}

impl MessageRouter {
    // Create router and spawn background task
    pub fn new() -> Self;

    // Register an actor with its mailbox
    pub async fn register_actor(&self, actor_id: TheaterId, mailbox_tx: Sender<ActorMessage>) -> Result<()>;

    // Unregister an actor
    pub async fn unregister_actor(&self, actor_id: TheaterId);

    // Route a message command
    pub async fn route_message(&self, command: MessageCommand) -> Result<()>;
}
```

### MessageCommand (Routing Commands)

```rust
pub enum MessageCommand {
    SendMessage {
        target_id: TheaterId,
        message: ActorMessage,
        response_tx: oneshot::Sender<Result<()>>,
    },
    OpenChannel {
        initiator_id: ChannelParticipant,
        target_id: ChannelParticipant,
        channel_id: ChannelId,
        initial_message: Vec<u8>,
        response_tx: oneshot::Sender<Result<bool>>,
    },
    ChannelMessage {
        channel_id: ChannelId,
        message: Vec<u8>,
        response_tx: oneshot::Sender<Result<()>>,
    },
    ChannelClose {
        channel_id: ChannelId,
        response_tx: oneshot::Sender<Result<()>>,
    },
}
```

### ActorMessage (Messages Delivered to Actors)

```rust
pub enum ActorMessage {
    Send(ActorSend { data: Vec<u8> }),
    Request(ActorRequest { data: Vec<u8>, response_tx: oneshot::Sender<Vec<u8>> }),
    ChannelOpen(ActorChannelOpen { channel_id, initiator_id, response_tx, initial_msg }),
    ChannelMessage(ActorChannelMessage { channel_id, message }),
    ChannelClose(ActorChannelClose { channel_id }),
}
```

## Current State

### ✅ What's Already Working

1. **MessageRouter is created** in `create_root_handler_registry()`:
   ```rust
   let message_router = MessageRouter::new();
   registry.register(MessageServerHandler::new(None, message_router));
   ```

2. **Actors are auto-registered** when they spawn (via MessageServerHandler's setup)

3. **Actors can message each other** using the handler's host functions:
   - `send(target_id, data)` - One-way send
   - `request(target_id, data)` - Request-response
   - Channel operations (open, send, close)

### ⚠️ What's Missing

The **TheaterServer** can't currently send messages to actors on behalf of external clients because:

1. Server doesn't have access to the MessageRouter instance
2. Old TheaterCommand variants were removed
3. No API bridge between external clients and MessageRouter

## Implementation Plan

### Phase 1: Expose MessageRouter to Server ⭐ (Critical)

**Goal:** Give TheaterServer access to the MessageRouter so it can route messages

**Approach 1: Pass MessageRouter to TheaterServer (Recommended)**

```rust
// In TheaterServer::new()
pub async fn new(address: std::net::SocketAddr) -> Result<Self> {
    let (theater_tx, theater_rx) = mpsc::channel(32);
    let (channel_events_tx, channel_events_rx) = mpsc::channel(32);

    // Create MessageRouter FIRST
    let message_router = MessageRouter::new();

    // Pass it to handler registry
    let handler_registry = create_root_handler_registry(
        theater_tx.clone(),
        message_router.clone(),  // ← Pass router
    );

    let runtime = TheaterRuntime::new(
        theater_tx.clone(),
        theater_rx,
        Some(channel_events_tx.clone()),
        handler_registry,
    ).await?;

    Ok(Self {
        runtime,
        theater_tx,
        management_socket,
        subscriptions: Arc::new(Mutex::new(HashMap::new())),
        channel_subscriptions,
        channel_events_tx,
        message_router,  // ← Store in server
    })
}
```

**Approach 2: Make MessageRouter a Static (Alternative)**

```rust
// Use a global static if you need access from multiple places
lazy_static! {
    static ref GLOBAL_MESSAGE_ROUTER: MessageRouter = MessageRouter::new();
}
```

**Recommendation:** Use Approach 1 (pass as parameter) for better testability and control.

### Phase 2: Implement SendActorMessage

**Goal:** Allow external clients to send one-way messages to actors

**Implementation:**

```rust
ManagementCommand::SendActorMessage { id, data } => {
    info!("Sending message to actor: {:?}", id);

    // Create response channel
    let (response_tx, response_rx) = oneshot::channel();

    // Create ActorMessage
    let message = ActorMessage::Send(ActorSend { data });

    // Route via MessageRouter
    self.message_router.route_message(MessageCommand::SendMessage {
        target_id: id.clone(),
        message,
        response_tx,
    }).await?;

    // Wait for routing result
    match response_rx.await? {
        Ok(()) => ManagementResponse::SentMessage { id },
        Err(e) => ManagementResponse::Error {
            error: ManagementError::RuntimeError(format!("Failed to send: {}", e)),
        },
    }
}
```

**Actor Side (WIT):**

```wit
// In actor's WIT file
import theater:simple/message-server-client;

// Actor implements:
export theater:simple/message-server-client.handle-send: func(data: list<u8>);
```

**Result:** External clients can send messages, actors receive them via `handle-send()`

### Phase 3: Implement RequestActorMessage

**Goal:** Allow external clients to make request-response calls to actors

**Implementation:**

```rust
ManagementCommand::RequestActorMessage { id, data } => {
    info!("Requesting message from actor: {:?}", id);

    // Create channels for request-response
    let (route_tx, route_rx) = oneshot::channel();
    let (response_tx, response_rx) = oneshot::channel();

    // Create ActorMessage with response channel
    let message = ActorMessage::Request(ActorRequest {
        data,
        response_tx,
    });

    // Route via MessageRouter
    self.message_router.route_message(MessageCommand::SendMessage {
        target_id: id.clone(),
        message,
        response_tx: route_tx,
    }).await?;

    // Wait for routing to complete
    route_rx.await??;

    // Wait for actor's response
    let response_data = response_rx.await
        .map_err(|e| anyhow::anyhow!("Actor didn't respond: {}", e))?;

    ManagementResponse::RequestedMessage {
        id,
        message: response_data,
    }
}
```

**Actor Side:**

```wit
// Actor implements request handler that returns data
export theater:simple/message-server-client.handle-request: func(data: list<u8>) -> list<u8>;
```

**Result:** External clients can make RPC-style calls to actors

### Phase 4: Implement Channel Operations

#### 4A: OpenChannel

**Goal:** Open bidirectional communication channel between external client and actor

**Implementation:**

```rust
ManagementCommand::OpenChannel { actor_id, initial_message } => {
    info!("Opening channel to actor: {:?}", actor_id);

    // Create channel ID
    let client_id = ChannelParticipant::External;
    let target_id = ChannelParticipant::Actor(actor_id.clone());
    let channel_id = ChannelId::new(&client_id, &target_id);

    // Create response channel
    let (response_tx, response_rx) = oneshot::channel();

    // Send channel open command via router
    self.message_router.route_message(MessageCommand::OpenChannel {
        initiator_id: client_id.clone(),
        target_id: target_id.clone(),
        channel_id: channel_id.clone(),
        initial_message,
        response_tx,
    }).await?;

    // Wait for actor to accept/reject
    match response_rx.await? {
        Ok(accepted) if accepted => {
            // Register channel for this client connection
            let channel_sub = ChannelSubscription {
                channel_id: channel_id.to_string(),
                initiator_id: client_id,
                target_id: target_id.clone(),
                client_tx: cmd_client_tx.clone(),
            };

            self.channel_subscriptions.lock().await
                .insert(channel_id.to_string(), channel_sub);

            ManagementResponse::ChannelOpened {
                channel_id: channel_id.to_string(),
                actor_id,
            }
        }
        Ok(false) => ManagementResponse::Error {
            error: ManagementError::ChannelRejected,
        },
        Err(e) => ManagementResponse::Error {
            error: ManagementError::RuntimeError(format!("Channel error: {}", e)),
        },
    }
}
```

**Actor Side:**

```wit
// Actor can accept or reject channel
export theater:simple/message-server-client.handle-channel-open: func(
    channel-id: string,
    initiator-id: string,
    initial-message: list<u8>
) -> bool;
```

#### 4B: SendOnChannel

**Goal:** Send messages on an established channel

**Implementation:**

```rust
ManagementCommand::SendOnChannel { channel_id, message } => {
    info!("Sending on channel: {}", channel_id);

    let (response_tx, response_rx) = oneshot::channel();

    self.message_router.route_message(MessageCommand::ChannelMessage {
        channel_id: ChannelId(channel_id.clone()),
        message,
        response_tx,
    }).await?;

    match response_rx.await? {
        Ok(()) => ManagementResponse::MessageSent { channel_id },
        Err(e) => ManagementResponse::Error {
            error: ManagementError::RuntimeError(format!("Send failed: {}", e)),
        },
    }
}
```

**Note:** ChannelMessage routing is marked as TODO in the MessageRouter. Needs implementation:

```rust
// In MessageRouter::handle_route_command()
MessageCommand::ChannelMessage { channel_id, message, response_tx } => {
    // TODO: Track open channels and route to correct participant
    // Need to implement channel state tracking
}
```

#### 4C: CloseChannel

**Goal:** Close a channel and cleanup state

**Implementation:**

```rust
ManagementCommand::CloseChannel { channel_id } => {
    info!("Closing channel: {}", channel_id);

    let (response_tx, response_rx) = oneshot::channel();

    self.message_router.route_message(MessageCommand::ChannelClose {
        channel_id: ChannelId(channel_id.clone()),
        response_tx,
    }).await?;

    // Remove from subscriptions
    self.channel_subscriptions.lock().await.remove(&channel_id);

    match response_rx.await? {
        Ok(()) => ManagementResponse::ChannelClosed { channel_id },
        Err(e) => ManagementResponse::Error {
            error: ManagementError::RuntimeError(format!("Close failed: {}", e)),
        },
    }
}
```

### Phase 5: Complete MessageRouter Channel Support

**Goal:** Implement the TODO items in MessageRouter for channel message routing

**Current State:**

```rust
// In MessageRouter::handle_route_command()
MessageCommand::ChannelMessage { channel_id, message, response_tx } => {
    // TODO: Implement channel message routing
    let _ = response_tx.send(Err(anyhow::anyhow!("Channel message routing not yet implemented")));
}

MessageCommand::ChannelClose { channel_id, response_tx } => {
    // TODO: Implement channel close routing
    let _ = response_tx.send(Err(anyhow::anyhow!("Channel close routing not yet implemented")));
}
```

**Implementation Needed:**

```rust
// Add channel state tracking to MessageRouter
struct ChannelState {
    initiator: ChannelParticipant,
    target: ChannelParticipant,
    is_open: bool,
}

// In router_task(), add:
let mut channels: HashMap<ChannelId, ChannelState> = HashMap::new();

// In handle_route_command():
MessageCommand::ChannelMessage { channel_id, message, response_tx } => {
    if let Some(channel) = channels.get(&channel_id) {
        // Route to appropriate participant based on sender
        // (Implementation depends on how you track which side is sending)
        todo!("Route message to correct participant");
    } else {
        let _ = response_tx.send(Err(anyhow::anyhow!("Channel not found")));
    }
}
```

### Phase 6: UpdateActorComponent (Lower Priority)

**Goal:** Hot-reload actor WASM components

**Challenge:** This is a separate feature from messaging, requires different approach

**Recommendation:** Defer this until messaging is complete. Possible approaches:

1. Add new `TheaterCommand::UpdateComponent`
2. Implement as actor operation via ActorHandle
3. Create separate component management service

## Implementation Order (Recommended)

1. ✅ **Phase 1** - Expose MessageRouter to Server (2-3 hours)
   - Modify `create_root_handler_registry()` to take router parameter
   - Store router in TheaterServer struct
   - Update TheaterServer::new()

2. ✅ **Phase 2** - SendActorMessage (1-2 hours)
   - Implement server-side routing
   - Test with simple actor
   - Update documentation

3. ✅ **Phase 3** - RequestActorMessage (1-2 hours)
   - Implement request-response pattern
   - Handle timeouts
   - Test with echo actor

4. ⚠️ **Phase 4A** - OpenChannel (2-3 hours)
   - Implement channel open
   - Handle accept/reject
   - Track channel subscriptions

5. ⚠️ **Phase 5** - Complete MessageRouter Channels (4-6 hours)
   - Implement channel state tracking in router
   - Implement ChannelMessage routing
   - Implement ChannelClose
   - Thorough testing

6. ⚠️ **Phase 4B/C** - SendOnChannel + CloseChannel (1 hour)
   - These become trivial once Phase 5 is done

7. ⏸️ **Phase 6** - UpdateActorComponent (TBD)
   - Design phase needed first
   - Can be deferred

## Testing Strategy

### Unit Tests

```rust
#[tokio::test]
async fn test_send_actor_message() {
    let router = MessageRouter::new();
    let (mailbox_tx, mut mailbox_rx) = mpsc::channel(10);

    // Register actor
    let actor_id = TheaterId::generate();
    router.register_actor(actor_id.clone(), mailbox_tx).await.unwrap();

    // Send message
    let (response_tx, response_rx) = oneshot::channel();
    router.route_message(MessageCommand::SendMessage {
        target_id: actor_id,
        message: ActorMessage::Send(ActorSend { data: vec![1, 2, 3] }),
        response_tx,
    }).await.unwrap();

    // Verify message received
    if let Some(ActorMessage::Send(msg)) = mailbox_rx.recv().await {
        assert_eq!(msg.data, vec![1, 2, 3]);
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_server_send_message() {
    // Start server
    let server = TheaterServer::new("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // Spawn echo actor
    let actor_id = /* spawn actor */;

    // Send message via management API
    let response = server.handle_command(ManagementCommand::SendActorMessage {
        id: actor_id,
        data: vec![42],
    }).await.unwrap();

    assert!(matches!(response, ManagementResponse::SentMessage { .. }));
}
```

## Migration Path

### For Existing Code

Old code using removed commands:

```rust
// OLD (doesn't compile)
runtime_tx.send(TheaterCommand::SendMessage {
    actor_id: id,
    actor_message: ActorMessage::Send(ActorSend { data }),
}).await?;
```

New code using MessageRouter:

```rust
// NEW (via server's MessageRouter)
let (response_tx, response_rx) = oneshot::channel();
self.message_router.route_message(MessageCommand::SendMessage {
    target_id: id,
    message: ActorMessage::Send(ActorSend { data }),
    response_tx,
}).await?;
response_rx.await??;
```

## Success Criteria

✅ **Phase 1-3 Complete:**
- External clients can send one-way messages to actors
- External clients can make request-response calls
- All basic messaging works

✅ **Phase 4-5 Complete:**
- External clients can open channels with actors
- Bidirectional communication works
- Channel lifecycle managed properly

✅ **Full Success:**
- All ManagementCommand variants implemented
- Comprehensive test coverage
- Documentation updated
- Performance benchmarks show >10k msgs/sec

## Estimated Timeline

- **Phases 1-3:** 1-2 days (basic messaging)
- **Phases 4-5:** 2-3 days (channels)
- **Phase 6:** TBD (component updates)

**Total for basic messaging:** ~1 week
**Total for full feature parity:** ~2 weeks

## Next Steps

1. Review this plan
2. Start with Phase 1 (expose MessageRouter)
3. Implement Phase 2-3 (basic messaging)
4. Test with real actors
5. Continue to channels when ready

This gives you a fully functional messaging system with the new architecture! 🚀
