# Theater Shutdown System Analysis

This document captures the current state of the shutdown system as of the analysis date.
It is intended to serve as a reference for refactoring efforts.

## Overview

The shutdown system spans three layers:
1. **Theater Runtime** - Orchestrates actor lifecycle, processes `TheaterCommand`s
2. **Actor Runtime** - Manages a single actor's execution loops and handlers
3. **Handlers** - Provide capabilities to actors (TCP, messaging, storage, etc.)

## Entry Points for Shutdown

### 1. Actor Self-Shutdown (`TheaterCommand::ShuttingDown`)

An actor can request its own shutdown via the runtime handler's `shutdown()` function.

```
Actor WASM code
  -> runtime handler shutdown() function
  -> spawns task to send TheaterCommand::ShuttingDown
  -> theater_runtime.shutdown_actor()
  -> notifies supervisor with ChildResult::Success
  -> calls stop_actor(Graceful)
```

Location: `theater_runtime.rs:1118-1137`

### 2. External Stop (`TheaterCommand::StopActor`)

External request to stop an actor (e.g., from CLI, API, or parent).

```
External request
  -> TheaterCommand::StopActor
  -> theater_runtime.stop_actor_external()
  -> notifies supervisor with ChildExternalStop
  -> calls stop_actor(Graceful)
```

Location: `theater_runtime.rs:430-447`, `1143-1164`

### 3. Force Terminate (`TheaterCommand::TerminateActor`)

Forceful termination of an actor.

```
External request
  -> TheaterCommand::TerminateActor
  -> stop_actor(Force)
```

Location: `theater_runtime.rs:449-462`

### 4. Error-Triggered Shutdown (`TheaterCommand::ActorError`)

Actor errors trigger automatic shutdown.

```
Actor error occurs
  -> TheaterCommand::ActorError
  -> theater_runtime.handle_actor_error()
  -> notifies supervisor with ChildError
  -> notifies subscribers
  -> calls stop_actor(Graceful)
```

Location: `theater_runtime.rs:863-906`

### 5. Full Runtime Shutdown (`TheaterCommand::ShutdownRuntime`)

Shuts down the entire theater runtime.

```
Runtime shutdown request
  -> TheaterCommand::ShutdownRuntime
  -> theater_runtime.stop()
  -> iterates all actors, calls stop_actor(Graceful) on each
  -> clears channel registrations
```

Location: `theater_runtime.rs:608-610`, `1347-1370`

---

## Core Shutdown Flow: `stop_actor()`

Location: `theater_runtime.rs:926-1115`

### Current Implementation (with issues noted):

```
1. Check actor exists in registry (contains_key)
2. Get actor reference (UNUSED - assigned to _proc)     <- REDUNDANT
3. Save "shutdown" event to chain
4. Save chain to disk
5. Remove chain from registry                           <- BEFORE children stopped
6. Get children list
7. Recursively stop all children (depth-first)
8. Get actor reference again                            <- 3rd lookup
9. Send ActorControl::Shutdown to actor runtime
10. Wait up to 10s for acknowledgment
11. Remove actor from registry
12. Signal proc.shutdown_controller                     <- DEAD CODE (no subscribers)
13. Remove actor from channel registrations
14. Clean up empty channels
```

### Issues Identified:

1. **Redundant existence checks**: Lines 932-944 check `contains_key` then `get()`, result unused
2. **Chain removed too early**: Chain removed at step 5, but children stopped at step 7
3. **Multiple actor lookups**: Actor looked up 4 times (steps 2, 6, 8, 11)
4. **Dead code**: `proc.shutdown_controller.signal_shutdown()` has no subscribers
5. **Doc comment incomplete**: Lists steps 1, 2, 5, 6 (missing 3, 4)

---

## Actor Runtime Shutdown

Location: `actor/runtime.rs:637-710`

When the actor runtime receives `ActorControl::Shutdown`:

```
1. Set phase to ShuttingDown
2. Signal handlers_shutdown_controller (THIS is what handlers listen to)
3. Wait for operation_handle, info_handle, setup_handle to finish
4. Send acknowledgment via response_tx
5. Break from control loop
6. Abort any remaining handler tasks
```

### Actor Runtime has its OWN ShutdownController

Created in `build_actor_resources()` at line 294:
```rust
let mut shutdown_controller = ShutdownController::new();
```

This controller is:
- Passed to `HandlerContext` for handlers to subscribe during `setup_host_functions_composite()`
- Used to subscribe each handler in the `setup()` loop (line 495)
- Stored in `handlers_shutdown_controller` and signaled on shutdown (line 648)

---

## The Two ShutdownController Problem

### Theater Runtime's Controller (UNUSED)

Location: `theater_runtime.rs:655-666`

```rust
let mut shutdown_controller = ShutdownController::new();
// ...
let shutdown_receiver = shutdown_controller.subscribe();
// ...
let _shutdown_receiver_clone = shutdown_receiver;  // DISCARDED!
```

This controller is stored in `ActorProcess` and signaled in `stop_actor()`:
```rust
proc.shutdown_controller.signal_shutdown(shutdown_type).await;  // NO SUBSCRIBERS!
```

### Actor Runtime's Controller (ACTUALLY USED)

Location: `actor/runtime.rs:294-297`

```rust
let mut shutdown_controller = ShutdownController::new();
let mut handler_ctx = HandlerContext::with_shutdown_controller(shutdown_controller.clone());
```

Handlers subscribe via:
1. `ctx.subscribe_shutdown()` during `setup_host_functions_composite()`
2. Automatic subscription in `setup()` loop (line 495)

This controller is signaled when `ActorControl::Shutdown` is received.

---

## Handler Shutdown Patterns

### Pattern 1: Simple Wait (Default)

```rust
fn run(&mut self, shutdown_receiver: ShutdownReceiver, ...) -> ... {
    Box::pin(async move {
        shutdown_receiver.wait_for_shutdown().await;
        Ok(())
    })
}
```

Used by: Runtime handler, simple handlers

### Pattern 2: Cancellation Token

```rust
fn setup(&mut self, ..., shutdown_receiver: ShutdownReceiver, ...) -> ... {
    let cancel_token = self.cancellation_token.clone();
    Box::pin(async move {
        shutdown_receiver.wait_for_shutdown().await;
        cancel_token.cancel();  // Cancel all spawned tasks
        Ok(())
    })
}
```

Used by: TCP handler

### Pattern 3: Select Loop

```rust
// In a spawned task
loop {
    tokio::select! {
        msg = receiver.recv() => { /* handle message */ }
        _ = &mut shutdown_receiver.receiver => {
            // Cleanup and break
            break;
        }
    }
}
```

Used by: Message server handler

### Pattern 4: Early Subscription

Some handlers need shutdown access during `setup_host_functions_composite()`:

```rust
fn setup_host_functions_composite(&mut self, builder, ctx) -> Result<()> {
    if let Some(shutdown_receiver) = ctx.subscribe_shutdown() {
        // Store for later use in spawned tasks
        *self.shutdown_receiver.lock().unwrap() = Some(shutdown_receiver);
    }
    // ... register functions ...
}
```

Used by: Message server handler

---

## ShutdownSignal RAII Pattern

Location: `shutdown.rs:15-29`

```rust
pub struct ShutdownSignal {
    pub shutdown_type: ShutdownType,
    sender: Option<Sender<()>>,
}

impl Drop for ShutdownSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());  // Auto-respond on drop
        }
    }
}
```

This ensures the shutdown controller doesn't wait indefinitely - when a handler
finishes processing (and drops the signal), it automatically acknowledges.

---

## ShutdownType

```rust
pub enum ShutdownType {
    Graceful,  // Wait for cleanup
    Force,     // Abort immediately
}
```

Currently:
- `Graceful` is used for most shutdowns
- `Force` is used for `TerminateActor` and `ActorControl::Terminate`

---

## Supervisor Notifications

Different shutdown paths send different notifications:

| Path | Notification |
|------|--------------|
| Self-shutdown | `ActorResult::Success(ChildResult)` |
| External stop | `ActorResult::ExternalStop(ChildExternalStop)` |
| Error | `ActorResult::Error(ChildError)` |

---

## Summary of Issues

1. **Dead ShutdownController in TheaterRuntime**: The `shutdown_controller` in `ActorProcess` has no subscribers. The receiver is discarded at creation.

2. **Redundant shutdown signaling**: `stop_actor()` sends `ActorControl::Shutdown` (which triggers handler shutdown inside actor runtime), then ALSO calls `proc.shutdown_controller.signal_shutdown()` (which does nothing).

3. **Ordering issues in stop_actor()**: Chain is saved/removed before children are stopped.

4. **Multiple actor lookups**: Same actor looked up 4 times in `stop_actor()`.

5. **Inconsistent patterns**: Some handlers use cancellation tokens, some use select loops, some just wait. No clear guidance on which to use when.

6. **Missing restart**: `restart_actor()` returns `Err("not implemented")`.

---

## Recommended Cleanup

1. **Remove theater runtime's ShutdownController from ActorProcess** - it's unused
2. **Consolidate actor lookups** in `stop_actor()` to a single lookup at the start
3. **Fix ordering** - stop children before touching the chain
4. **Update documentation** to match actual implementation
5. **Standardize handler patterns** with clear guidance
