# Handler Integration Guide

This guide explains how to integrate Theater handlers at different stages of the runtime lifecycle.

## Handler Integration Timeline

Handlers can be integrated at different points depending on their dependencies:

### 1. ✅ Handler Registry Creation (10/11 handlers)

These handlers can be registered when creating the `HandlerRegistry`, before the runtime starts:

| Handler | Dependencies | When to Register |
|---------|--------------|------------------|
| **environment** | Config, Permissions | Registry creation |
| **random** | Config, Permissions | Registry creation |
| **timing** | Config, Permissions | Registry creation |
| **runtime** | Config, `theater_tx`, Permissions | Registry creation |
| **http-client** | Config, Permissions | Registry creation |
| **filesystem** | Config, Permissions | Registry creation |
| **store** | Config, Permissions | Registry creation |
| **supervisor** | Config, Permissions | Registry creation |
| **message-server** | Config, MessageRouter, Permissions | Registry creation |
| **http-framework** | Permissions | Registry creation |

**Key Insight:** Even though `RuntimeHandler` needs the `theater_tx` channel, we create that channel *before* creating the handler registry, so it's available at registration time!

```rust
// Create channels first
let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);

// Create registry and pass theater_tx to handlers that need it
let mut registry = HandlerRegistry::new();
registry.register(RuntimeHandler::new(
    RuntimeHostConfig {},
    theater_tx.clone(),  // ✅ Available!
    None
));

// Then create runtime
let runtime = TheaterRuntime::new(theater_tx, theater_rx, None, registry).await?;
```

### 2. ⚠️ Actor Creation Time (1/11 handlers)

This handler requires dependencies that only exist when spawning actors:

| Handler | Blocker | When to Integrate |
|---------|---------|-------------------|
| **process** | Requires `ActorHandle` (created during actor spawn) | Per-actor integration |

**Why ProcessHandler is Different:**

```rust
pub fn new(
    config: ProcessHostConfig,
    actor_handle: ActorHandle,  // ❌ Only exists during actor spawning
    permissions: Option<ProcessPermissions>,
) -> Self
```

The `ActorHandle` is created by the runtime when spawning an actor, not during handler registry setup. Each actor needs its own ProcessHandler instance with its specific ActorHandle.

**Solution:** ProcessHandler must be integrated during the actor spawning process, typically in TheaterServer or custom actor creation logic.

## Common Misconceptions

### ❌ "Handlers needing runtime resources can't be registered early"

**Reality:** Many handlers need runtime resources like `theater_tx`, but these are created *before* the handler registry, so they're available at registration time.

### ❌ "All handlers are created per-actor"

**Reality:** Handlers registered in the HandlerRegistry are shared across actors (via `create_instance()` cloning). Only handlers requiring per-actor state (like ProcessHandler needing an ActorHandle) must be created per-actor.

### ✅ "Handler dependencies determine integration point"

**Correct:** The key question is "When does this dependency become available?"
- Available before runtime creation? → Register in HandlerRegistry
- Available only during actor creation? → Integrate per-actor

## Integration Examples

### Example 1: Standard Runtime (10/11 handlers)

```rust
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use tokio::sync::mpsc;

async fn create_runtime() -> Result<()> {
    // 1. Create channels
    let (theater_tx, theater_rx) = mpsc::channel(32);

    // 2. Create handler registry
    let mut registry = HandlerRegistry::new();

    // Register all compatible handlers
    registry.register(EnvironmentHandler::new(env_config, None));
    registry.register(RandomHandler::new(random_config, None));
    registry.register(TimingHandler::new(timing_config, None));
    registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None)); // ✅
    registry.register(HttpClientHandler::new(http_config, None));
    registry.register(FilesystemHandler::new(fs_config, None));
    registry.register(StoreHandler::new(store_config, None));
    registry.register(SupervisorHandler::new(supervisor_config, None));

    let message_router = MessageRouter::new();
    registry.register(MessageServerHandler::new(None, message_router));
    registry.register(HttpFrameworkHandler::new(None));

    // 3. Create runtime
    let runtime = TheaterRuntime::new(
        theater_tx,
        theater_rx,
        None,
        registry,
    ).await?;

    // 4. Run runtime
    runtime.run().await?;

    Ok(())
}
```

### Example 2: Per-Actor ProcessHandler (TheaterServer approach)

ProcessHandler integration happens in the actor spawning logic:

```rust
// This happens inside TheaterRuntime::spawn_actor() or similar
async fn spawn_actor_with_process_handler(
    manifest: ManifestConfig,
    theater_tx: Sender<TheaterCommand>,
) -> Result<TheaterId> {
    // 1. Create actor handle
    let actor_handle = ActorHandle::new(/* ... */);

    // 2. Create ProcessHandler with actor's handle
    let process_handler = ProcessHandler::new(
        ProcessHostConfig::default(),
        actor_handle.clone(),  // ✅ Now available
        None,
    );

    // 3. Add to actor's handler list
    actor_handlers.push(Box::new(process_handler));

    // 4. Continue actor creation...
    Ok(actor_id)
}
```

## Summary

- **10/11 handlers** can be registered in the HandlerRegistry at runtime creation
- **1/11 handlers** (ProcessHandler) requires per-actor integration
- The key is understanding *when* each dependency becomes available
- RuntimeHandler was incorrectly assumed to need per-actor integration, but it only needs `theater_tx` which is created before the handler registry

## See Also

- `/crates/theater/examples/full-runtime.rs` - Working example with 10/11 handlers
- `/crates/theater-server/` - Complete server implementation with all 11 handlers
