# Message-Server Handler Migration Blocker

**Status**: Handler code complete, but **blocked on core theater infrastructure**

**Date**: 2025-12-08

## Problem Summary

The message-server handler has been fully implemented (~1,315 lines) with all operations, state management, and message processing. However, it **cannot compile** because required `TheaterCommand` enum variants are missing from the core theater crate.

## What Was Completed

✅ **Handler Implementation** (~1,315 lines)
- `MessageServerHandler` struct with all state management
- Background message processing loop (handles all 6 `ActorMessage` types)
- All 5 export functions (handle-send, handle-request, handle-channel-open, handle-channel-message, handle-channel-close)
- Complete error handling and event recording

✅ **All 8 Operations Implemented**:
1. `send` - One-way message to another actor
2. `request` - Request-response RPC pattern
3. `list-outstanding-requests` - Query pending requests
4. `respond-to-request` - Respond to incoming request
5. `cancel-request` - Cancel pending request
6. `open-channel` - Create bidirectional channel
7. `send-on-channel` - Send message over channel
8. `close-channel` - Close channel

✅ **State Management**:
- `Arc<Mutex<HashMap<ChannelId, ChannelState>>>` - Track open channels
- `Arc<Mutex<HashMap<String, oneshot::Sender<Vec<u8>>>>>` - Pending requests
- `Arc<Mutex<Option<Receiver<ActorMessage>>>>` - Mailbox pattern (like supervisor)

## The Blocker

### Missing TheaterCommand Variants

The handler code uses these `TheaterCommand` variants that **don't exist** in `/crates/theater/src/messages.rs`:

```rust
// Used in operations 1-2 (send, request):
TheaterCommand::SendMessage {
    actor_id: TheaterId,
    actor_message: ActorMessage,  // ActorMessage::Send or ActorMessage::Request
}

// Used in operation 6 (open-channel):
TheaterCommand::ChannelOpen {
    initiator_id: ChannelParticipant,
    target_id: ChannelParticipant,
    channel_id: ChannelId,
    initial_message: Vec<u8>,
    response_tx: oneshot::Sender<Result<bool>>,
}

// Used in operation 7 (send-on-channel):
TheaterCommand::ChannelMessage {
    channel_id: ChannelId,
    message: Vec<u8>,
}

// Used in operation 8 (close-channel):
TheaterCommand::ChannelClose {
    channel_id: ChannelId,
}
```

### Struct Field Mismatches

Additionally, message structs have different field names than expected:

**`ActorChannelOpen`** (in `/crates/theater/src/messages.rs:639`):
```rust
pub struct ActorChannelOpen {
    pub channel_id: ChannelId,
    pub response_tx: oneshot::Sender<Result<bool>>,
    pub data: Vec<u8>,  // Handler expects: initial_msg
    // Handler also expects: initiator_id field (missing)
}
```

**`ActorChannelMessage`** (in `/crates/theater/src/messages.rs:662`):
```rust
pub struct ActorChannelMessage {
    pub channel_id: ChannelId,
    pub data: Vec<u8>,  // Handler expects: msg
}
```

### Helper Method Missing

**`ChannelId`** lacks a `parse()` method:
```rust
// Handler uses but doesn't exist:
ChannelId::parse(&channel_id_str)
```

## Compilation Errors

```
error[E0599]: no variant named `SendMessage` found for enum `TheaterCommand`
error[E0599]: no variant named `ChannelOpen` found for enum `TheaterCommand`
error[E0599]: no variant named `ChannelMessage` found for enum `TheaterCommand`
error[E0599]: no variant named `ChannelClose` found for enum `TheaterCommand`
error[E0599]: no function or associated item named `parse` found for struct `ChannelId`
error[E0026]: struct `ActorChannelOpen` does not have fields named `initiator_id`, `initial_msg`
error[E0027]: pattern does not mention field `data`
error[E0026]: struct `ActorChannelMessage` does not have a field named `msg`
error[E0027]: pattern does not mention field `data`
```

Total: **11 compilation errors**

## Why This Happened

During the earlier handler migrations, the messaging infrastructure was likely:
1. Never fully implemented in the core `TheaterCommand` enum, OR
2. Removed/refactored but the old `message_server.rs` still referenced the old API

The original `/crates/theater/src/host/message_server.rs` (1,280 lines) uses these variants, which suggests they existed at some point or were planned but never added.

## Solutions

### Option 1: Add Missing Infrastructure to Core Theater (Recommended)

**File**: `/crates/theater/src/messages.rs`

Add to `TheaterCommand` enum:
```rust
pub enum TheaterCommand {
    // ... existing variants ...

    /// Send a message to an actor's mailbox
    SendMessage {
        actor_id: TheaterId,
        actor_message: ActorMessage,
    },

    /// Open a bidirectional channel between actors
    ChannelOpen {
        initiator_id: ChannelParticipant,
        target_id: ChannelParticipant,
        channel_id: ChannelId,
        initial_message: Vec<u8>,
        response_tx: oneshot::Sender<Result<bool>>,
    },

    /// Send a message over an existing channel
    ChannelMessage {
        channel_id: ChannelId,
        message: Vec<u8>,
    },

    /// Close a channel
    ChannelClose {
        channel_id: ChannelId,
    },
}
```

Update struct field names:
```rust
// In ActorChannelOpen
pub struct ActorChannelOpen {
    pub channel_id: ChannelId,
    pub initiator_id: ChannelParticipant,  // Add
    pub response_tx: oneshot::Sender<Result<bool>>,
    pub initial_msg: Vec<u8>,  // Rename from: data
}

// In ActorChannelMessage
pub struct ActorChannelMessage {
    pub channel_id: ChannelId,
    pub msg: Vec<u8>,  // Rename from: data
}
```

Add helper method:
```rust
impl ChannelId {
    pub fn parse(s: &str) -> Result<Self> {
        // Parse channel ID from string representation
        // Implementation depends on ChannelId structure
    }
}
```

Then add handler for these commands in `TheaterRuntime`:
- Route `SendMessage` to actor's mailbox
- Route `ChannelOpen/Message/Close` to appropriate actors

### Option 2: Refactor Handler to Use Different Routing

Instead of using `TheaterCommand`, directly route messages through actor mailboxes. This would require:
- Access to the theater runtime's actor registry
- Direct mailbox sending instead of command-based routing
- More coupling between handler and runtime internals

**Not recommended** - defeats the purpose of the modular handler architecture.

### Option 3: Defer Message-Server Migration

Leave message-server in the core `theater` crate for now and complete the other handler (http-framework). The messaging system may need more architectural design before extraction.

## Impact

**Cannot complete migration without**:
- Adding 4 `TheaterCommand` variants
- Updating 2 struct definitions
- Adding 1 helper method
- Implementing command handlers in `TheaterRuntime`

**Estimated work**: 2-4 hours to add infrastructure + integration testing

## Files

**Handler (complete)**:
- `/crates/theater-handler-message-server/src/lib.rs` (1,315 lines)
- `/crates/theater-handler-message-server/Cargo.toml` (dependencies configured)

**Core changes needed**:
- `/crates/theater/src/messages.rs` (add variants, update structs, add parse method)
- `/crates/theater/src/theater_runtime.rs` (handle new command variants)

## Recommendation

**Add the missing infrastructure** to properly support actor-to-actor messaging. The message-server handler is architecturally sound and complete - it just needs the core theater crate to provide the command routing infrastructure.

This is a good opportunity to design the messaging system properly as a first-class feature of the theater runtime.

## Next Steps

1. ✅ Document this blocker (this file)
2. ✅ Update migration tracking to show message-server as "BLOCKED"
3. Consider moving to http-framework handler (last Phase 4 handler)
4. Return to message-server after infrastructure is added
