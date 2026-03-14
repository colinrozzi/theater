# Shutdown Resource Ownership

This document analyzes what resources each level owns and must clean up during shutdown.

## Design Principle

Each level should:
1. Clean up its own resources
2. Signal its children to shut down
3. Wait for children to complete
4. Report completion to its parent

The shutdown should propagate **down** the tree, and completion should propagate **up**.

---

## Level 1: Handlers

### Resources Owned

Each handler may own different resources depending on its purpose:

| Handler | Resources |
|---------|-----------|
| **TCP** | Listener tasks, connection tasks, open sockets, CancellationToken |
| **Message Server** | Consumer task, router registration, message subscriptions |
| **Timer** | Scheduled timer tasks |
| **Store** | Content store references (shared, no cleanup needed) |
| **Supervisor** | Channel to parent (just drop) |
| **Runtime** | None significant |
| **RPC** | RPC listener tasks |
| **Terminal** | Terminal I/O handles |

### Cleanup Responsibilities

1. **Cancel/abort background tasks** - Stop listeners, consumers, timers
2. **Close connections** - TCP sockets, etc.
3. **Unregister from shared resources** - Message router, etc.
4. **Flush pending work** (graceful only) - Send pending messages, etc.

### Shutdown Interface

Handlers receive shutdown via `ShutdownReceiver`:
```rust
fn setup(..., shutdown_receiver: ShutdownReceiver, ...) -> Future<Result<()>>
```

The returned future should:
- Wait for `shutdown_receiver.wait_for_shutdown()`
- Perform cleanup
- Return `Ok(())` when done

The `ShutdownSignal` auto-responds when dropped (RAII).

---

## Level 2: Actor Runtime

### Resources Owned

| Resource | Location | Description |
|----------|----------|-------------|
| `actor_instance_wrapper` | `Arc<RwLock<Option<PackInstance>>>` | The WASM instance |
| `handler_tasks` | `Vec<JoinHandle<()>>` | Spawned handler setup futures |
| `handlers_shutdown_controller` | `ShutdownController` | Signals handlers |
| `setup_handle` | `JoinHandle` | Actor setup task |
| `info_handle` | `JoinHandle` | Info loop task |
| `operation_handle` | `JoinHandle` | Operation loop task |
| `metrics` | `Arc<RwLock<MetricsCollector>>` | Performance metrics |
| `control_rx` | `Receiver<ActorControl>` | Control channel (owned) |

### Cleanup Responsibilities

1. **Signal handlers to shut down** - via `handlers_shutdown_controller`
2. **Wait for operation/info loops to complete** - They check `ActorPhase::ShuttingDown`
3. **Wait for handler tasks to complete** - Or abort if taking too long
4. **Drop the WASM instance** - Releases memory
5. **Respond to parent** - via `response_tx` in `ActorControl::Shutdown`

### Shutdown Interface

Actor runtime receives shutdown via `ActorControl::Shutdown`:
```rust
ActorControl::Shutdown { response_tx: oneshot::Sender<Result<(), ActorError>> }
```

### Current Flow (actor/runtime.rs:640-668)

```rust
ActorControl::Shutdown { response_tx } => {
    // 1. Set phase (stops loops from accepting new work)
    actor_phase_manager.set_phase(ActorPhase::ShuttingDown);

    // 2. Signal handlers
    handlers_shutdown_controller.signal_shutdown(Graceful).await;

    // 3. Wait for loops to finish
    tokio::join!(operation_handle, info_handle, setup_handle);

    // 4. Respond to parent
    response_tx.send(Ok(()));

    // 5. Break from control loop (task ends, resources dropped)
    break;
}
```

---

## Level 3: Theater Runtime

### Resources Owned (per actor)

| Resource | Field | Description |
|----------|-------|-------------|
| `ActorProcess` | `actors` | Contains channels and metadata |
| `process` | `ActorProcess.process` | JoinHandle for actor runtime task |
| `control_tx` | `ActorProcess.control_tx` | Send shutdown command |
| `operation_tx` | `ActorProcess.operation_tx` | For pending operations |
| `info_tx` | `ActorProcess.info_tx` | For info queries |
| `mailbox_tx` | `ActorProcess.mailbox_tx` | For messages |
| `children` | `ActorProcess.children` | Child actor IDs |
| `StateChain` | `chains` | Event chain for the actor |
| Channel registrations | `channels` | Inter-actor communication |
| Subscriptions | `subscriptions` | Event subscribers |

### Cleanup Responsibilities

1. **Stop children first** - Depth-first shutdown of supervision tree
2. **Send shutdown to actor runtime** - via `ActorControl::Shutdown`
3. **Wait for actor runtime to complete** - With timeout
4. **Record shutdown event** - In the chain
5. **Save the chain** - Persist to disk
6. **Remove from registries** - actors, chains, channels, subscriptions
7. **Notify supervisor** - If this actor has a parent

### Shutdown Interface

Theater runtime receives commands via `TheaterCommand`:
- `StopActor` - External request to stop
- `TerminateActor` - Force stop
- `ShuttingDown` - Actor self-shutdown
- `ShutdownRuntime` - Stop everything

---

## Proposed Shutdown Order

### For a Single Actor

```
Theater Runtime                    Actor Runtime                 Handlers
      |                                  |                           |
      | 1. Stop children (recursive)     |                           |
      |                                  |                           |
      | 2. ActorControl::Shutdown ------>|                           |
      |                                  | 3. Set ShuttingDown phase |
      |                                  |                           |
      |                                  | 4. Signal handlers ------>|
      |                                  |                           | 5. Cancel tasks
      |                                  |                           | 6. Close connections
      |                                  |                           | 7. Cleanup
      |                                  |<---- 8. Handler futures complete
      |                                  |                           |
      |                                  | 9. Wait for loops         |
      |                                  |                           |
      |<-- 10. response_tx.send(Ok) -----|                           |
      |                                  | 11. Break, task ends      |
      |                                  |                           |
      | 12. Record shutdown event        |                           |
      | 13. Save chain                   |                           |
      | 14. Remove from registries       |                           |
      | 15. Notify supervisor            |                           |
      |                                  |                           |
```

### Key Invariants

1. **Children before parent** - All children must be stopped before parent cleanup
2. **Handlers before actor** - All handlers must complete before actor runtime ends
3. **Actor before theater** - Actor runtime must respond before theater removes it
4. **Chain before removal** - Chain must be saved before actor is removed from registry

---

## Questions to Resolve

1. **When should the chain be saved?**
   - Current: Before children are stopped
   - Proposed: After actor runtime confirms shutdown, before removal

2. **Should theater runtime's ShutdownController be removed?**
   - It's currently unused (receiver discarded)
   - Actor runtime has its own controller for handlers

3. **What's the timeout policy?**
   - Current: 10s for actor runtime response, no handler timeout
   - Should handlers have individual timeouts?

4. **Force vs Graceful at each level?**
   - Current: Only passed to `signal_shutdown()`
   - Should loops abort immediately on Force?

5. **What about pending operations?**
   - Operations in flight when shutdown starts
   - Should they complete or be cancelled?

---

## Current vs Proposed Architecture

### Current (Problematic)

```
TheaterRuntime
├── ShutdownController (UNUSED - receiver discarded)
└── ActorProcess
    └── sends ActorControl::Shutdown
        └── ActorRuntime
            └── ShutdownController (handlers subscribe here)
```

### Proposed (Simplified)

```
TheaterRuntime
└── ActorProcess
    └── sends ActorControl::Shutdown
        └── ActorRuntime
            └── ShutdownController (handlers subscribe here)
```

Remove the unused `ShutdownController` from `ActorProcess` entirely.
