# ProcessHandler Analysis: Why It Needs ActorHandle

## The Problem

ProcessHandler is the only handler that **cannot** be registered at handler registry creation time because it requires an `ActorHandle`, which is only created during actor spawning.

## What Does ProcessHandler Do?

ProcessHandler allows actors to spawn OS processes and interact with them. Here's the flow:

```
Actor (WASM)
    ↓ calls os-spawn()
ProcessHandler
    ↓ spawns
OS Process
    ↓ produces stdout/stderr
Background Tasks (tokio)
    ↓ reads output
    ↓ needs to call back into actor!
    ↓ uses ActorHandle.call_function()
Actor (WASM)
    ↓ handle-stdout(process_id, data)
    ↓ handle-stderr(process_id, data)
    ↓ handle-exit(process_id, exit_code)
```

## Why ActorHandle Is Needed

When an OS process produces output, ProcessHandler needs to **call back into the actor** to deliver that output. It does this via:

```rust
actor_handle.call_function::<(u64, Vec<u8>), ()>(
    "theater:simple/process-handlers/handle-stdout",
    (process_id, data)
).await
```

### ActorHandle Contents

```rust
pub struct ActorHandle {
    operation_tx: mpsc::Sender<ActorOperation>,  // Call functions on actor
    info_tx: mpsc::Sender<ActorInfo>,           // Get state, chain, metrics
    control_tx: mpsc::Sender<ActorControl>,     // Shutdown, etc.
}
```

These channels connect directly to a specific actor instance's runtime. They're created during `ActorRuntime::start()`, not during handler registration.

## The Three Callbacks ProcessHandler Uses

From `/crates/theater-handler-process/src/lib.rs`:

1. **handle-stdout** (line 576)
   ```rust
   actor_handle.call_function::<(u64, Vec<u8>), ()>(
       "theater:simple/process-handlers/handle-stdout",
       (process_id, stdout_data)
   )
   ```

2. **handle-stderr** (line 598)
   ```rust
   actor_handle.call_function::<(u64, Vec<u8>), ()>(
       "theater:simple/process-handlers/handle-stderr",
       (process_id, stderr_data)
   )
   ```

3. **handle-exit** (line 668)
   ```rust
   actor_handle.call_function::<(u64, i32), ()>(
       "theater:simple/process-handlers/handle-exit",
       (process_id, exit_code)
   )
   ```

## Current Architecture

```rust
// In TheaterRuntime::spawn_actor()
let (operation_tx, operation_rx) = mpsc::channel(100);
let (info_tx, info_rx) = mpsc::channel(100);
let (control_tx, control_rx) = mpsc::channel(100);

// Create actor handle (ONLY available here!)
let actor_handle = ActorHandle::new(operation_tx, info_tx, control_tx);

// Now we can create ProcessHandler
let process_handler = ProcessHandler::new(config, actor_handle, permissions);
```

## Possible Solutions

### Option 1: Lazy Initialization ⭐ (Recommended)

Change ProcessHandler to accept the ActorHandle later:

```rust
pub struct ProcessHandler {
    config: ProcessHostConfig,
    processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    next_process_id: Arc<Mutex<u64>>,
    actor_handle: Arc<RwLock<Option<ActorHandle>>>,  // ← Optional!
    permissions: Option<ProcessPermissions>,
}

impl ProcessHandler {
    pub fn new(config: ProcessHostConfig, permissions: Option<ProcessPermissions>) -> Self {
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            next_process_id: Arc::new(Mutex::new(1)),
            actor_handle: Arc::new(RwLock::new(None)),  // ← Start empty
            permissions,
        }
    }

    pub fn set_actor_handle(&mut self, handle: ActorHandle) {
        *self.actor_handle.write().unwrap() = Some(handle);
    }
}

impl Handler for ProcessHandler {
    fn start(&mut self, actor_handle: ActorHandle, shutdown: ShutdownReceiver)
        -> Pin<Box<dyn Future<Output = Result<()>> + Send>>
    {
        // Set the handle when the handler starts!
        self.set_actor_handle(actor_handle);

        Box::pin(async { /* ... */ })
    }
}
```

**Benefits:**
- Can register in HandlerRegistry at startup ✅
- Still gets ActorHandle when needed ✅
- Minimal changes to architecture ✅

**Trade-offs:**
- Need to handle Option<ActorHandle> in code
- Slight runtime overhead checking if handle is set

### Option 2: Pass ActorHandle in start()

The Handler trait already passes ActorHandle to `start()`:

```rust
fn start(
    &mut self,
    actor_handle: ActorHandle,  // ← Already available!
    shutdown_receiver: ShutdownReceiver,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
```

ProcessHandler could store it when `start()` is called:

```rust
pub struct ProcessHandler {
    config: ProcessHostConfig,
    processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    next_process_id: Arc<Mutex<u64>>,
    actor_handle: Arc<RwLock<Option<ActorHandle>>>,  // ← Optional
    permissions: Option<ProcessPermissions>,
}

impl Handler for ProcessHandler {
    fn start(&mut self, actor_handle: ActorHandle, shutdown: ShutdownReceiver)
        -> Pin<Box<dyn Future<Output = Result<()>> + Send>>
    {
        // Store the handle!
        *self.actor_handle.write().unwrap() = Some(actor_handle);

        // Continue with normal start logic
        Box::pin(async move { /* ... */ })
    }
}
```

**Benefits:**
- Uses existing Handler trait API ✅
- No new methods needed ✅
- Can register in HandlerRegistry ✅

**Trade-offs:**
- Same Option<ActorHandle> handling needed

### Option 3: Callback Registry Pattern

Create a shared registry for actor callbacks:

```rust
// Global or runtime-scoped registry
pub struct ActorCallbackRegistry {
    handles: Arc<RwLock<HashMap<TheaterId, ActorHandle>>>,
}

impl ActorCallbackRegistry {
    pub fn register(&self, actor_id: TheaterId, handle: ActorHandle) {
        self.handles.write().unwrap().insert(actor_id, handle);
    }

    pub fn get(&self, actor_id: &TheaterId) -> Option<ActorHandle> {
        self.handles.read().unwrap().get(actor_id).cloned()
    }
}

pub struct ProcessHandler {
    config: ProcessHostConfig,
    processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
    callback_registry: Arc<ActorCallbackRegistry>,  // ← Shared registry
    permissions: Option<ProcessPermissions>,
}
```

**Benefits:**
- Cleaner separation of concerns
- Could be used by other handlers too

**Trade-offs:**
- More complex architecture
- Need to track actor IDs
- More indirection

### Option 4: Split ProcessHandler

Separate process management from callback handling:

```rust
// Core handler - can be registered early
pub struct ProcessManager {
    config: ProcessHostConfig,
    processes: Arc<Mutex<HashMap<u64, ManagedProcess>>>,
}

// Per-actor callback wrapper
pub struct ProcessCallbackHandler {
    manager: Arc<ProcessManager>,
    actor_handle: ActorHandle,
}
```

**Benefits:**
- Clear separation
- Manager can be shared

**Trade-offs:**
- More complex
- Breaks existing handler pattern

## Recommendation

**Go with Option 1 or 2** (they're very similar):

1. Make `actor_handle` field in ProcessHandler `Option<ActorHandle>`
2. Store it in the `start()` method (which already receives it!)
3. Update process spawning code to use the stored handle

This is the minimal change that allows ProcessHandler to be registered in the HandlerRegistry like other handlers, while still getting the ActorHandle it needs when the actor starts.

## Implementation Checklist

- [ ] Change ProcessHandler constructor to not require ActorHandle
- [ ] Add `actor_handle: Arc<RwLock<Option<ActorHandle>>>` field
- [ ] Store ActorHandle in `start()` method
- [ ] Update all `actor_handle` usages to unwrap the Option
- [ ] Add error handling for cases where handle isn't set yet
- [ ] Update example to include ProcessHandler in registry
- [ ] Test with actual process spawning

## References

- ProcessHandler: `/crates/theater-handler-process/src/lib.rs`
- ActorHandle: `/crates/theater/src/actor/handle.rs`
- Handler trait: `/crates/theater/src/handler/mod.rs`
