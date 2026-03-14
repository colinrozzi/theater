# Handler Shutdown Analysis

This document analyzes each handler's resources and shutdown behavior to design proper tests.

---

## Handler Summary Table

| Handler | Background Tasks | Resources to Clean Up | Current Shutdown | Issues |
|---------|-----------------|----------------------|------------------|--------|
| **runtime** | None | None | Wait for signal | OK |
| **store** | None | None (filesystem ops are sync) | Wait for signal | OK |
| **supervisor** | Child result loop | `channel_rx` receiver | Select loop with shutdown | OK |
| **tcp** | Listener tasks, active mode tasks | Connections, listeners, CancellationToken | Cancel token | **Connections not closed explicitly** |
| **message-server** | Message consumption loop | Router registration, mailbox | Select loop, unregister | OK, but **only if register() called** |
| **timer** | Per-timer tasks | Active timers map | Cancel all timers | OK |
| **loop** | Main loop task | Shutdown receiver | Notify pattern | **Complex handoff** |
| **terminal** | Input reading task | Shutdown receiver | Notify pattern | **Complex handoff** |
| **rpc** | None | None | Wait for signal | OK |

---

## Detailed Analysis

### 1. Runtime Handler

**Location:** `crates/theater-handler-runtime/src/lib.rs`

**Resources:** None

**Shutdown behavior:**
```rust
fn setup(..., shutdown_receiver, ...) {
    Box::pin(async {
        shutdown_receiver.wait_for_shutdown().await;
        Ok(())
    })
}
```

**Test needed:** Basic - verify setup future completes when shutdown signaled.

---

### 2. Store Handler

**Location:** `crates/theater-handler-store/src/lib.rs`

**Resources:** None (creates `ContentStore` per-call, filesystem operations are atomic)

**Shutdown behavior:** Same as runtime - just waits for signal.

**Test needed:** Basic - verify no hanging operations.

---

### 3. Supervisor Handler

**Location:** `crates/theater-handler-supervisor/src/lib.rs:699-729`

**Resources:**
- `channel_rx: mpsc::Receiver<ActorResult>` - receives child lifecycle events

**Shutdown behavior:**
```rust
fn setup(..., mut shutdown_receiver, ...) {
    let channel_rx = self.channel_rx.lock().unwrap().take();
    Box::pin(async move {
        let Some(mut channel_rx) = channel_rx else { return Ok(()); };
        loop {
            tokio::select! {
                Some(child_result) = channel_rx.recv() => { /* process */ }
                _ = &mut shutdown_receiver.receiver => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }
        Ok(())
    })
}
```

**Issues:** None - properly breaks from loop on shutdown.

**Test needed:**
1. Shutdown while child results are pending
2. Shutdown with no children spawned

---

### 4. TCP Handler

**Location:** `crates/theater-handler-tcp/src/lib.rs:373-400`

**Resources:**
- `shared_state.connections: HashMap<u64, ConnectionEntry>` - open TCP connections
- `shared_state.listeners: HashMap<u64, ListenerEntry>` - TCP listeners
- `cancellation_token: CancellationToken` - for spawned tasks

**Shutdown behavior:**
```rust
fn setup(..., shutdown_receiver, ...) {
    let cancel_token = self.cancellation_token.clone();
    Box::pin(async move {
        shutdown_receiver.wait_for_shutdown().await;
        cancel_token.cancel();
        Ok(())
    })
}
```

**Issues:**
1. **Connections not explicitly closed** - relies on drop, but streams are in `shared_state`
2. **Listeners not explicitly closed** - same issue
3. **Active mode tasks** - should be cancelled by token, but connection cleanup unclear

**Test needed:**
1. Shutdown with open connections - verify they are closed
2. Shutdown with active listener - verify listener stops
3. Shutdown during active mode data transfer

**Proposed fix:**
```rust
Box::pin(async move {
    shutdown_receiver.wait_for_shutdown().await;

    // Close all connections
    let mut conns = shared_state.connections.lock().await;
    for (id, entry) in conns.drain() {
        if let StreamState::Full(stream) = entry.stream {
            // stream.shutdown() or just drop
        }
    }

    // Close all listeners
    let mut listeners = shared_state.listeners.lock().await;
    listeners.clear(); // dropping TcpListener closes it

    cancel_token.cancel();
    Ok(())
})
```

---

### 5. Message Server Handler

**Location:** `crates/theater-handler-message-server/src/lib.rs:563-598, 706-738`

**Resources:**
- Router registration (external)
- Mailbox channel
- Background consumption task

**Shutdown behavior:**

Setup stores the receiver but may not use it:
```rust
fn setup(..., shutdown_receiver, ...) {
    // Store shutdown_receiver in mutex
    // Wait for register() to be called
    Box::pin(async move {
        registered_notify.notified().await;
        Ok(())
    })
}
```

In `register()`:
```rust
tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = &mut shutdown_receiver.receiver => {
                info!("Shutdown signal received");
                break;
            }
            Some(msg) = mailbox_rx.recv() => { /* process */ }
        }
    }
    // Unregister from router
    router.unregister_actor(actor_id).await;
});
```

**Issues:**
1. **If `register()` never called** - shutdown_receiver stays in mutex, setup() waits forever on `registered_notify`
2. **Outstanding requests** not cleaned up explicitly

**Test needed:**
1. Shutdown after register() - verify unregistration
2. Shutdown without register() - **this will hang!**
3. Shutdown with pending requests

**Proposed fix for setup():**
```rust
Box::pin(async move {
    tokio::select! {
        _ = registered_notify.notified() => {
            info!("Registered, setup complete");
        }
        _ = alternative_shutdown_wait() => {
            info!("Shutdown before register, cleaning up");
        }
    }
    Ok(())
})
```

---

### 6. Timer Handler

**Location:** `crates/theater-handler-timer/src/lib.rs:119-153`

**Resources:**
- `active_timers: HashMap<String, mpsc::Sender<()>>` - cancel senders for timers
- Per-timer spawned tasks

**Shutdown behavior:**
```rust
fn setup(..., shutdown_receiver, ...) {
    let state = self.state.clone();
    Box::pin(async move {
        shutdown_receiver.wait_for_shutdown().await;

        // Cancel all active timers
        if let Some(ref s) = state {
            let timers = s.active_timers.lock().await;
            for (name, cancel_tx) in timers.iter() {
                let _ = cancel_tx.send(()).await;
            }
        }
        Ok(())
    })
}
```

**Issues:** None - properly cancels all timers.

**Test needed:**
1. Shutdown with active timers - verify all cancelled
2. Verify timer tasks actually exit

---

### 7. Loop Handler

**Location:** `crates/theater-handler-loop/src/lib.rs:165-190`

**Resources:**
- Shutdown receiver (may be taken by `start-loop()`)
- Main loop task

**Shutdown behavior:** Complex handoff pattern similar to message-server.

**Issues:**
- If `start-loop()` never called, what happens?

**Test needed:**
1. Shutdown after start-loop()
2. Shutdown without start-loop()

---

### 8. Terminal Handler

**Location:** `crates/theater-handler-terminal/src/lib.rs:221-246`

**Resources:**
- Shutdown receiver (may be taken by `enable-input()`)
- Input reading task

**Shutdown behavior:** Complex handoff pattern.

**Issues:**
- Same as loop handler

**Test needed:**
1. Shutdown after enable-input()
2. Shutdown without enable-input()

---

### 9. RPC Handler

**Location:** `crates/theater-handler-rpc/src/lib.rs:131-146`

**Resources:** None

**Shutdown behavior:** Just waits for signal.

**Test needed:** Basic verification.

---

## Test Design

### Test Infrastructure Needed

```rust
/// Test harness for handler shutdown testing
struct HandlerShutdownTest {
    shutdown_controller: ShutdownController,
    handler: Box<dyn Handler>,
    setup_future: Pin<Box<dyn Future<Output = Result<()>> + Send>>,
}

impl HandlerShutdownTest {
    async fn new<H: Handler>(handler: H) -> Self {
        let mut shutdown_controller = ShutdownController::new();
        let shutdown_receiver = shutdown_controller.subscribe();

        let mut handler = Box::new(handler);
        let setup_future = handler.setup(
            mock_actor_handle(),
            mock_actor_instance(),
            shutdown_receiver,
            mock_event_rx(),
        );

        Self { shutdown_controller, handler, setup_future }
    }

    /// Signal shutdown and verify setup completes within timeout
    async fn shutdown_and_verify(self, timeout: Duration) -> Result<()> {
        let shutdown_future = self.shutdown_controller.signal_shutdown(ShutdownType::Graceful);

        tokio::select! {
            _ = self.setup_future => Ok(()),
            _ = tokio::time::sleep(timeout) => {
                Err(anyhow!("Handler did not shutdown within {:?}", timeout))
            }
        }
    }
}
```

### Per-Handler Tests

#### TCP Handler Tests

```rust
#[tokio::test]
async fn test_tcp_shutdown_closes_connections() {
    let handler = TcpHandler::new(TcpHandlerConfig::default());
    let test = HandlerShutdownTest::new(handler).await;

    // Simulate an open connection
    // ... setup connection via host functions ...

    // Verify connection count before shutdown
    assert_eq!(handler.shared_state.connections.lock().await.len(), 1);

    // Shutdown
    test.shutdown_and_verify(Duration::from_secs(5)).await?;

    // Verify connection closed
    assert_eq!(handler.shared_state.connections.lock().await.len(), 0);
}

#[tokio::test]
async fn test_tcp_shutdown_with_active_listener() {
    // Similar pattern
}
```

#### Message Server Handler Tests

```rust
#[tokio::test]
async fn test_message_server_shutdown_after_register() {
    // Setup, call register(), then shutdown
    // Verify unregistration from router
}

#[tokio::test]
async fn test_message_server_shutdown_without_register() {
    // Setup, immediately shutdown
    // Currently this will HANG - test should fail/timeout
    // After fix, should complete cleanly
}
```

---

## Priority Order for Fixes

1. **Message Server** - Can hang on shutdown (HIGH)
2. **TCP** - Connections not explicitly closed (MEDIUM)
3. **Loop/Terminal** - May have similar hang issues (MEDIUM)
4. **Others** - Generally OK

---

## Next Steps

1. Create test infrastructure in `crates/theater/src/handler/tests/`
2. Write failing tests for known issues
3. Fix handlers one by one
4. Ensure all tests pass
