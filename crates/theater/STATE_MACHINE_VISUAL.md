# Actor State Machine Visual Guide

## Current Implementation (Implicit State)

```
┌─────────────────────────────────────────────────────────────┐
│                      Giant Select Loop                      │
│                                                             │
│  • actor_instance: Option<...>                             │
│  • metrics: Option<...>                                     │
│  • handler_tasks: Vec<...>                                  │
│  • current_operation: Option<...>                           │
│  • shutdown_requested: bool                                 │
│  • shutdown_response_tx: Option<...>                        │
│  • current_status: String                                   │
│  • paused: Arc<RwLock<bool>>                               │
│                                                             │
│  ┌────────────────────────────────────────────────┐       │
│  │ tokio::select! {                               │       │
│  │   setup completion => { ... }                  │       │
│  │   operation recv => { if guards... }           │       │
│  │   operation complete => { ... }                │       │
│  │   info request => { ... }                      │       │
│  │   control command => { if this, if that... }   │       │
│  │   parent shutdown => { ... }                   │       │
│  │   status update => { ... }                     │       │
│  │ }                                              │       │
│  └────────────────────────────────────────────────┘       │
│                                                             │
│  State is implicit - hard to reason about!                 │
└─────────────────────────────────────────────────────────────┘
```

**Problems:**
- Can't tell at a glance what state we're in
- Boolean flags interact in complex ways
- Guards on select branches hide logic
- Testing is difficult

---

## Proposed Implementation (Explicit State)

```
                    ┌──────────────┐
                    │   Starting   │
                    │              │
                    │ • setup_task │
                    │ • status_rx  │
                    └──────┬───────┘
                           │
                    setup complete
                           │
                           ▼
                    ┌──────────────┐◄────────┐
           ┌────────┤     Idle     │         │
           │        │              │         │
           │        │ • resources  │         │
  pause    │        └──────┬───────┘         │
           │               │                 │
           │        new operation    operation
           │               │           complete
           │               ▼                 │
           │        ┌──────────────┐         │
           ├───────►│  Processing  ├─────────┘
           │        │              │
           │        │ • resources  │
           │        │ • operation  │
  resume   │        └──────┬───────┘
           │               │
           │       shutdown/error
           │               │
           │               ▼
           │        ┌──────────────┐
           └───────►│    Paused    │
                    │              │
                    │ • resources  │
                    └──────┬───────┘
                           │
                    shutdown/error
                           │
                           ▼
                    ┌──────────────┐
                    │ ShuttingDown │
                    │              │
                    │  (terminal)  │
                    └──────────────┘
```

**Benefits:**
- States are explicit and named
- Transitions are clear arrows
- Each state has only what it needs
- Easy to reason about!

---

## State Details

### Starting State
```rust
Starting {
    setup_task: JoinHandle<Result<SetupComplete, ActorError>>,
    status_rx: Receiver<String>,
    current_status: String,
    pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
}
```

**What it means:**
- Actor is initializing
- Loading WASM, setting up handlers
- Can receive: status updates, info requests, control commands
- Cannot receive: operations (not ready yet!)
- Transitions to: Idle (success) or ShuttingDown (error/terminate)

**Key insight:** If shutdown is requested during startup, we store the response channel and complete shutdown after setup finishes.

---

### Idle State
```rust
Idle {
    resources: ActorResources,
}

struct ActorResources {
    instance: Arc<RwLock<ActorInstance>>,
    metrics: Arc<RwLock<MetricsCollector>>,
    handler_tasks: Vec<JoinHandle<()>>,
    shutdown_controller: ShutdownController,
}
```

**What it means:**
- Actor is ready and waiting
- Has all resources initialized
- Can receive: operations, info requests, control commands
- Transitions to: Processing (new operation), Paused (pause), ShuttingDown (shutdown)

**Key insight:** All resources are guaranteed to exist - no Option unwrapping!

---

### Processing State
```rust
Processing {
    resources: ActorResources,
    current_operation: JoinHandle<Result<Vec<u8>, ActorError>>,
    operation_name: String,
    pending_shutdown: Option<oneshot::Sender<Result<(), ActorError>>>,
}
```

**What it means:**
- Actor is executing an operation
- Cannot accept new operations
- Can receive: info requests, control commands
- Transitions to: Idle (operation complete), ShuttingDown (terminate)

**Key insight:** If shutdown is requested during operation, we store it and complete after operation finishes (graceful) or abort immediately (forced).

---

### Paused State
```rust
Paused {
    resources: ActorResources,
}
```

**What it means:**
- Actor is paused by user
- Will not accept operations
- Can receive: info requests, control commands
- Transitions to: Idle (resume), ShuttingDown (shutdown)

**Key insight:** Simple state - just holds resources and waits for resume.

---

### ShuttingDown State
```rust
ShuttingDown
```

**What it means:**
- Actor is cleaning up
- Terminal state (exit the loop)
- No data needed - cleanup happens in transition

**Key insight:** This is just a marker - actual cleanup is done before entering this state.

---

## Message Handling by State

### Starting State
| Message | Action |
|---------|--------|
| Setup complete | → Idle (or ShuttingDown if pending) |
| Setup failed | → ShuttingDown (notify error) |
| Info request | Handle (some work, some don't) |
| Operation | ❌ Ignored (not ready) |
| Pause | ❌ Reject (can't pause during startup) |
| Shutdown (graceful) | Mark pending, → Idle after setup |
| Shutdown (forced) | Abort setup, → ShuttingDown |

### Idle State
| Message | Action |
|---------|--------|
| Operation | Start operation, → Processing |
| Info request | Handle and stay in Idle |
| Pause | → Paused |
| Resume | ❌ Reject (not paused) |
| Shutdown | → ShuttingDown |

### Processing State
| Message | Action |
|---------|--------|
| Operation complete | → Idle (or ShuttingDown if pending) |
| Operation | ❌ Ignored (already processing) |
| Info request | Handle and stay in Processing |
| Pause | ❌ Reject (can't pause during operation) |
| Shutdown (graceful) | Mark pending, → ShuttingDown after operation |
| Shutdown (forced) | Abort operation, → ShuttingDown |

### Paused State
| Message | Action |
|---------|--------|
| Operation | ❌ Ignored (paused) |
| Info request | Handle and stay in Paused |
| Pause | Acknowledge (already paused) |
| Resume | → Idle |
| Shutdown | → ShuttingDown |

---

## Transition Logic

### From Starting
```rust
match setup_task.await {
    Ok(Ok(setup)) => {
        let resources = create_resources(setup);
        if pending_shutdown.is_some() {
            return StateTransition::Shutdown;
        }
        StateTransition::Continue(ActorState::Idle { resources })
    }
    Ok(Err(error)) => StateTransition::Error(error),
    Err(panic) => StateTransition::Error(ActorError::Panic),
}
```

### From Idle
```rust
match message {
    Operation => StateTransition::Continue(ActorState::Processing { ... }),
    Pause => StateTransition::Continue(ActorState::Paused { resources }),
    Shutdown => StateTransition::Shutdown,
    _ => StateTransition::Continue(ActorState::Idle { resources }),
}
```

### From Processing
```rust
match event {
    OperationComplete => {
        if pending_shutdown.is_some() {
            StateTransition::Shutdown
        } else {
            StateTransition::Continue(ActorState::Idle { resources })
        }
    }
    Shutdown(Graceful) => StateTransition::Continue(ActorState::Processing {
        pending_shutdown: Some(response_tx),
        ..
    }),
    Shutdown(Force) => {
        operation.abort();
        StateTransition::Shutdown
    }
}
```

### From Paused
```rust
match message {
    Resume => StateTransition::Continue(ActorState::Idle { resources }),
    Shutdown => StateTransition::Shutdown,
    _ => StateTransition::Continue(ActorState::Paused { resources }),
}
```

---

## Code Size Comparison

### Current Implementation
```
start() method:        ~400 lines
└─ select! block:      ~350 lines
   ├─ setup branch:     ~50 lines
   ├─ status branch:    ~5 lines
   ├─ operation branch: ~60 lines
   ├─ complete branch:  ~15 lines
   ├─ info branch:      ~80 lines
   ├─ control branch:   ~80 lines
   └─ shutdown branch:  ~60 lines
```

### Refactored Implementation
```
run() loop:                        ~40 lines
├─ handle_starting_state():        ~80 lines
│  └─ Focused on startup logic
├─ handle_idle_state():            ~60 lines
│  └─ Focused on accepting work
├─ handle_processing_state():      ~70 lines
│  └─ Focused on operation execution
└─ handle_paused_state():          ~40 lines
   └─ Focused on pause/resume

Total: ~290 lines, but MUCH more readable!
```

---

## Testing Comparison

### Current: Hard to Test
```rust
// How do you test "pause during operation"?
// You need to:
// 1. Set up entire runtime
// 2. Mock channels
// 3. Send operation
// 4. Wait for it to start
// 5. Send pause
// 6. Check... what? The boolean flag?
```

### Refactored: Easy to Test
```rust
#[tokio::test]
async fn test_pause_during_processing() {
    let state = ActorState::Processing {
        resources: test_resources(),
        current_operation: test_operation(),
        operation_name: "test".into(),
        pending_shutdown: None,
    };
    
    let (tx, rx) = oneshot::channel();
    send_control(Control::Pause { response_tx: tx });
    
    let transition = handle_processing_state(state).await;
    
    // Should reject pause during operation
    assert!(matches!(
        transition,
        StateTransition::Continue(ActorState::Processing { .. })
    ));
    assert!(rx.await.unwrap().is_err());
}
```

---

## Debugging Experience

### Current: Hard to Debug
```
[DEBUG] Operation received
[DEBUG] Starting operation
... (something goes wrong)
[ERROR] Failed to send response
```

*Where were we in the state machine? Was setup complete? Was there already an operation running?*

### Refactored: Easy to Debug
```
[INFO] Actor abc123: Starting -> Idle
[INFO] Actor abc123: Idle -> Processing (operation: calculate)
[DEBUG] Operation 'calculate' started
... (something goes wrong)
[ERROR] Operation 'calculate' failed: division by zero
[INFO] Actor abc123: Processing -> Idle
```

*State transitions are explicit! Easy to see what was happening.*

---

## Summary

The explicit state machine:

✅ **Clearer:** States and transitions are obvious
✅ **Safer:** Impossible states can't be represented  
✅ **Testable:** Each state handler can be tested independently
✅ **Maintainable:** Adding new states is straightforward
✅ **Debuggable:** State transitions are logged explicitly

The current implementation:

❌ States are implicit (boolean flags)
❌ Easy to get into invalid states
❌ Hard to test (need full runtime setup)
❌ Hard to extend (where does new logic go?)
❌ Hard to debug (can't see current state)

**This is a high-leverage refactor that will make your life much easier!**
