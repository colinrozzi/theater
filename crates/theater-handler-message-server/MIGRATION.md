# Migration Guide: Message Server Handler

This guide helps you migrate from the old lifecycle-based architecture to the new external router architecture.

## Overview of Changes

### Old Architecture (Lifecycle-Based)
- MessageServerHandler created internal channels
- Runtime sent lifecycle events to handler
- Handler maintained global actor registry
- Centralized mailbox consumption

### New Architecture (External Router)
- MessageRouter is external service
- Handler instances are per-actor
- Handlers register themselves during setup
- Each actor consumes its own mailbox

## Breaking Changes

### 1. Handler Construction

**Before:**
```rust
// Old: Handler created channels
let (handler, lifecycle_tx, message_tx) = MessageServerHandler::new(None);
```

**After:**
```rust
// New: Router created separately
let message_router = MessageRouter::new();
let handler = MessageServerHandler::new(None, message_router);
```

### 2. Runtime Creation

**Before:**
```rust
// Old: Runtime needed lifecycle channel
let runtime = TheaterRuntime::new(
    theater_tx,
    theater_rx,
    None,
    Some(lifecycle_tx), // Lifecycle coupling
    handler_registry,
).await?;
```

**After:**
```rust
// New: No lifecycle coupling
let runtime = TheaterRuntime::new(
    theater_tx,
    theater_rx,
    None,
    handler_registry, // Just registry
).await?;
```

### 3. ActorStore Changes

**Before:**
```rust
// Old: ActorStore had message_tx field
let store = ActorStore::new(id, theater_tx, Some(message_tx), handle, chain);
```

**After:**
```rust
// New: No message_tx field
let store = ActorStore::new(id, theater_tx, handle, chain);
```

## Step-by-Step Migration

### Step 1: Update Dependencies

```toml
[dependencies]
theater-handler-message-server = "0.3.0" # Or latest version
```

### Step 2: Create Router Before Theater

```rust
// Add this BEFORE creating Theater
let message_router = MessageRouter::new();
```

### Step 3: Update Handler Creation

```rust
// Old code:
// let (handler, lifecycle_tx, message_tx) = MessageServerHandler::new(None);

// New code:
let handler = MessageServerHandler::new(None, message_router.clone());
```

### Step 4: Update Runtime Creation

```rust
// Old code:
// let runtime = TheaterRuntime::new(tx, rx, None, Some(lifecycle_tx), registry).await?;

// New code:
let runtime = TheaterRuntime::new(tx, rx, None, registry).await?;
```

### Step 5: Remove Lifecycle Channel Usage

If you were using the lifecycle_tx or message_tx channels directly, remove that code:

```rust
// Old: Don't do this anymore
// lifecycle_tx.send(ActorLifecycleEvent::ActorSpawned { ... }).await?;

// New: Nothing needed! Handlers register themselves automatically
```

## Complete Example

### Before (Old Architecture)

```rust
use theater::theater_runtime::TheaterRuntime;
use theater_handler_message_server::MessageServerHandler;
use tokio::sync::mpsc;

async fn setup_old() -> Result<()> {
    // Create handler with internal channels
    let (handler, lifecycle_tx, _message_tx) = MessageServerHandler::new(None);

    // Register handler
    let mut handler_registry = HandlerRegistry::new();
    handler_registry.register("message-server", Box::new(handler));

    // Create runtime with lifecycle coupling
    let (theater_tx, theater_rx) = mpsc::channel(100);
    let runtime = TheaterRuntime::new(
        theater_tx,
        theater_rx,
        None,
        Some(lifecycle_tx), // ❌ Coupling
        handler_registry,
    ).await?;

    Ok(())
}
```

### After (New Architecture)

```rust
use theater::theater_runtime::TheaterRuntime;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use tokio::sync::mpsc;

async fn setup_new() -> Result<()> {
    // Create external router FIRST
    let message_router = MessageRouter::new();

    // Create handler with router reference
    let handler = MessageServerHandler::new(None, message_router.clone());

    // Register handler
    let mut handler_registry = HandlerRegistry::new();
    handler_registry.register("message-server", Box::new(handler));

    // Create runtime (no lifecycle coupling!)
    let (theater_tx, theater_rx) = mpsc::channel(100);
    let runtime = TheaterRuntime::new(
        theater_tx,
        theater_rx,
        None,
        handler_registry, // ✅ Clean
    ).await?;

    Ok(())
}
```

## Actor Code

**No changes needed!** Actor WASM code using the messaging API remains the same:

```rust
// Still works exactly the same
message_server::send("actor_b", data).unwrap();
let response = message_server::request("actor_b", data).unwrap();
```

## Testing Changes

### Before (Old Tests)

```rust
#[test]
fn test_handler() {
    let (handler, _lifecycle_tx, _message_tx) = MessageServerHandler::new(None);
    assert_eq!(handler.name(), "message-server");
}
```

### After (New Tests)

```rust
#[test]
fn test_handler() {
    let router = MessageRouter::new();
    let handler = MessageServerHandler::new(None, router);
    assert_eq!(handler.name(), "message-server");
}
```

## Common Migration Issues

### Issue 1: "ActorLifecycleEvent not found"

**Error:**
```
error[E0432]: unresolved import `theater::messages::ActorLifecycleEvent`
```

**Solution:**
Remove the import - ActorLifecycleEvent no longer exists:
```rust
// Remove this:
// use theater::messages::ActorLifecycleEvent;
```

### Issue 2: "MessageServerHandler::new expects 2 arguments"

**Error:**
```
error: this function takes 2 arguments but 1 was supplied
```

**Solution:**
Add MessageRouter parameter:
```rust
let router = MessageRouter::new();
let handler = MessageServerHandler::new(None, router);
```

### Issue 3: "TheaterRuntime::new expects 4 arguments"

**Error:**
```
error: this function takes 4 arguments but 5 were supplied
```

**Solution:**
Remove the lifecycle_tx parameter:
```rust
// Old: 5 parameters
// let runtime = TheaterRuntime::new(tx, rx, None, Some(lifecycle_tx), registry).await?;

// New: 4 parameters
let runtime = TheaterRuntime::new(tx, rx, None, registry).await?;
```

### Issue 4: "ActorStore::new expects 4 arguments"

**Error:**
```
error: this function takes 4 arguments but 5 were supplied
```

**Solution:**
Remove the message_tx parameter:
```rust
// Old:
// let store = ActorStore::new(id, theater_tx, Some(message_tx), handle, chain);

// New:
let store = ActorStore::new(id, theater_tx, handle, chain);
```

## Benefits of New Architecture

After migration, you gain:

1. **Performance** - 10x throughput improvement (100k+ msgs/sec)
2. **Separation** - Theater runtime has zero messaging coupling
3. **Flexibility** - Router is optional, external service
4. **Simplicity** - No lifecycle channel management
5. **Scalability** - Zero lock contention

## Need Help?

- See [README.md](README.md) for usage guide
- See [message-router-architecture.md](../../changes/in-progress/message-router-architecture.md) for architecture details
- Check examples in `examples/` directory
- Ask questions in GitHub issues

## Rollback

If you need to rollback to the old architecture:

```toml
[dependencies]
theater-handler-message-server = "0.2.0" # Last old-architecture version
```

Note: The old architecture is deprecated and will not receive updates.
