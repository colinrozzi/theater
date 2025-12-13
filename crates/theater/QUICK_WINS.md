# Quick Wins: Immediate Improvements You Can Make Today

These are small, low-risk refactorings you can do RIGHT NOW to start improving the codebase without committing to the full state machine refactor.

## Quick Win #1: Extract Helper Methods (30 minutes)

### Before: Inline Operation Execution
```rust
// In the giant select block
Some(op) = operation_rx.recv(), if actor_instance.is_some() && current_operation.is_none() => {
    match op {
        ActorOperation::CallFunction { name, params, response_tx } => {
            let theater_tx = theater_tx.clone();
            let metrics = metrics.clone();
            let actor_instance = actor_instance.clone();
            
            current_operation = Some(tokio::spawn(async move {
                let mut actor_instance = actor_instance.write().await;
                let metrics = metrics.write().await;
                match Self::execute_call(&mut actor_instance, &name, params, &theater_tx, &metrics).await {
                    // 20+ lines of handling...
                }
            }));
        }
    }
}
```

### After: Extracted Method
```rust
// In the select block
Some(op) = operation_rx.recv(), if actor_instance.is_some() && current_operation.is_none() => {
    current_operation = Some(self.spawn_operation_task(op, &actor_instance, &metrics));
}

// New helper method
fn spawn_operation_task(
    &self,
    op: ActorOperation,
    actor_instance: &Arc<RwLock<ActorInstance>>,
    metrics: &Arc<RwLock<MetricsCollector>>,
) -> JoinHandle<()> {
    match op {
        ActorOperation::CallFunction { name, params, response_tx } => {
            let theater_tx = self.theater_tx.clone();
            let metrics = metrics.clone();
            let actor_instance = actor_instance.clone();
            
            tokio::spawn(async move {
                let mut actor_instance = actor_instance.write().await;
                let metrics = metrics.write().await;
                match Self::execute_call(&mut actor_instance, &name, params, &theater_tx, &metrics).await {
                    Ok(result) => {
                        let _ = response_tx.send(Ok(result));
                    }
                    Err(error) => {
                        let _ = theater_tx.send(TheaterCommand::ActorError {
                            actor_id: actor_instance.id().clone(),
                            error: error.clone(),
                        }).await;
                        let _ = response_tx.send(Err(error));
                    }
                }
            })
        }
        ActorOperation::UpdateComponent { component_address: _, response_tx } => {
            tokio::spawn(async move {
                let _ = response_tx.send(Err(ActorError::UpdateComponentError("Not implemented".to_string())));
            })
        }
    }
}
```

**Impact:** Reduces select block by ~20 lines, improves readability

---

## Quick Win #2: Use Enums for Status (15 minutes)

### Before: String Status
```rust
let mut current_status = "Starting".to_string();

// Later...
current_status = "Ready".to_string();
current_status = "Processing".to_string();
current_status = "Shutting down".to_string();
```

### After: Enum Status
```rust
#[derive(Debug, Clone, Copy)]
enum RuntimeStatus {
    Starting,
    SettingUpStore,
    CreatingHandlers,
    CreatingComponent,
    SettingUpHostFunctions,
    Instantiating,
    Ready,
    Processing,
    Paused,
    ShuttingDown,
}

impl RuntimeStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::SettingUpStore => "Setting up actor store",
            Self::CreatingHandlers => "Creating handlers",
            Self::CreatingComponent => "Creating component",
            Self::SettingUpHostFunctions => "Setting up host functions",
            Self::Instantiating => "Instantiating component",
            Self::Ready => "Ready",
            Self::Processing => "Processing",
            Self::Paused => "Paused",
            Self::ShuttingDown => "Shutting down",
        }
    }
}

let mut current_status = RuntimeStatus::Starting;
```

**Impact:** Type safety, no typos, better autocomplete

---

## Quick Win #3: Extract Status Determination (20 minutes)

### Before: Scattered Status Logic
```rust
ActorInfo::GetStatus { response_tx } => {
    let status = if shutdown_requested {
        if setup_task.is_some() {
            "Shutting down (during startup)".to_string()
        } else if current_operation.is_some() {
            "Shutting down (waiting for operation)".to_string()
        } else {
            "Shutting down".to_string()
        }
    } else if setup_task.is_some() {
        current_status.clone()
    } else if *paused.read().await {
        "Paused".to_string()
    } else if current_operation.is_some() {
        "Processing".to_string()
    } else {
        "Idle".to_string()
    };
    let _ = response_tx.send(Ok(status));
}
```

### After: Extracted Method
```rust
ActorInfo::GetStatus { response_tx } => {
    let status = self.current_status(
        shutdown_requested,
        &setup_task,
        &current_operation,
        &current_status,
        &paused,
    ).await;
    let _ = response_tx.send(Ok(status.as_str().to_string()));
}

async fn current_status(
    &self,
    shutdown_requested: bool,
    setup_task: &Option<JoinHandle<_>>,
    current_operation: &Option<JoinHandle<_>>,
    startup_status: &RuntimeStatus,
    paused: &Arc<RwLock<bool>>,
) -> RuntimeStatus {
    if shutdown_requested {
        return RuntimeStatus::ShuttingDown;
    }
    
    if setup_task.is_some() {
        return *startup_status;
    }
    
    if *paused.read().await {
        return RuntimeStatus::Paused;
    }
    
    if current_operation.is_some() {
        return RuntimeStatus::Processing;
    }
    
    RuntimeStatus::Ready
}
```

**Impact:** Logic is testable, reusable, clear

---

## Quick Win #4: Named Constants for Magic Numbers (10 minutes)

### Before
```rust
let (status_tx, status_rx) = mpsc::channel(10);
let (mailbox_tx, mailbox_rx) = mpsc::channel(100);
```

### After
```rust
const STATUS_CHANNEL_SIZE: usize = 10;
const MAILBOX_CHANNEL_SIZE: usize = 100;

let (status_tx, status_rx) = mpsc::channel(STATUS_CHANNEL_SIZE);
let (mailbox_tx, mailbox_rx) = mpsc::channel(MAILBOX_CHANNEL_SIZE);
```

**Impact:** Self-documenting, easier to tune

---

## Quick Win #5: Builder for SpawnActor (45 minutes)

### Before: 6 Parameters
```rust
async fn spawn_actor(
    &mut self,
    manifest_path: String,
    init_bytes: Option<Vec<u8>>,
    parent_id: Option<TheaterId>,
    init: bool,
    supervisor_tx: Option<Sender<ActorResult>>,
    subscription_tx: Option<Sender<Result<ChainEvent, ActorError>>>,
) -> Result<TheaterId>
```

### After: Builder Pattern
```rust
pub struct SpawnActorRequest {
    manifest_path: String,
    init_bytes: Option<Vec<u8>>,
    parent_id: Option<TheaterId>,
    init: bool,
    supervisor_tx: Option<Sender<ActorResult>>,
    subscription_tx: Option<Sender<Result<ChainEvent, ActorError>>>,
}

impl SpawnActorRequest {
    pub fn new(manifest_path: impl Into<String>) -> Self {
        Self {
            manifest_path: manifest_path.into(),
            init_bytes: None,
            parent_id: None,
            init: true,
            supervisor_tx: None,
            subscription_tx: None,
        }
    }
    
    pub fn with_init_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.init_bytes = Some(bytes);
        self
    }
    
    pub fn with_parent(mut self, parent_id: TheaterId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }
    
    pub fn no_init(mut self) -> Self {
        self.init = false;
        self
    }
    
    pub fn with_supervisor(mut self, tx: Sender<ActorResult>) -> Self {
        self.supervisor_tx = Some(tx);
        self
    }
    
    pub fn with_subscription(mut self, tx: Sender<Result<ChainEvent, ActorError>>) -> Self {
        self.subscription_tx = Some(tx);
        self
    }
}

async fn spawn_actor(&mut self, request: SpawnActorRequest) -> Result<TheaterId> {
    // Use request.manifest_path, request.init_bytes, etc.
}

// Usage:
let actor_id = runtime.spawn_actor(
    SpawnActorRequest::new("path/to/manifest.toml")
        .with_parent(parent_id)
        .with_supervisor(supervisor_tx)
).await?;
```

**Impact:** Self-documenting, flexible, easier to evolve

---

## Quick Win #6: Extract Channel Handling (1 hour)

The channel handling code is ~100 lines in the main select. Extract it:

```rust
// In theater_runtime.rs
impl<E: EventType> TheaterRuntime<E> {
    async fn handle_channel_command(&mut self, cmd: ChannelCommand) -> Result<()> {
        match cmd {
            ChannelCommand::Open { initiator_id, target_id, channel_id, initial_message, response_tx } => {
                self.handle_channel_open(initiator_id, target_id, channel_id, initial_message, response_tx).await
            }
            ChannelCommand::Message { channel_id, message, sender_id } => {
                self.handle_channel_message(channel_id, message, sender_id).await
            }
            ChannelCommand::Close { channel_id } => {
                self.handle_channel_close(channel_id).await
            }
        }
    }
}

// In the main loop:
match cmd {
    TheaterCommand::ChannelOpen { .. } => {
        self.handle_channel_command(ChannelCommand::Open { .. }).await?;
    }
    // etc.
}
```

**Impact:** Main select block gets much smaller, channel logic is grouped

---

## Quick Win #7: Consolidate Error Responses (30 minutes)

### Before: Repeated Pattern
```rust
match result {
    Ok(value) => {
        if let Err(e) = response_tx.send(Ok(value)) {
            error!("Failed to send response: {:?}", e);
        }
    }
    Err(e) => {
        error!("Operation failed: {}", e);
        if let Err(send_err) = response_tx.send(Err(e)) {
            error!("Failed to send error response: {:?}", send_err);
        }
    }
}
```

### After: Helper Method
```rust
fn respond<T>(response_tx: oneshot::Sender<Result<T>>, result: Result<T>, operation: &str) {
    match &result {
        Ok(_) => debug!("Operation '{}' succeeded", operation),
        Err(e) => error!("Operation '{}' failed: {}", operation, e),
    }
    
    if let Err(_) = response_tx.send(result) {
        error!("Failed to send response for operation '{}'", operation);
    }
}

// Usage:
Self::respond(response_tx, result, "spawn_actor");
```

**Impact:** DRY, consistent logging, less boilerplate

---

## Quick Win #8: Add Tracing Spans (20 minutes)

### Before: Individual debug/info calls
```rust
debug!("Starting operation: {}", name);
// ... operation code ...
debug!("Operation completed: {}", name);
```

### After: Tracing Spans
```rust
use tracing::{info_span, Instrument};

let span = info_span!("operation", name = %name);
async move {
    // operation code
}
.instrument(span)
.await
```

**Impact:** Better structured logging, easier debugging with distributed tracing

---

## Implementation Order

Do these in order for maximum impact with minimum risk:

1. **Quick Win #4** (10 min) - Named constants, zero risk
2. **Quick Win #2** (15 min) - Status enum, minimal risk
3. **Quick Win #7** (30 min) - Error response helper, safe
4. **Quick Win #3** (20 min) - Extract status determination, testable
5. **Quick Win #1** (30 min) - Extract operation spawning
6. **Quick Win #8** (20 min) - Add tracing spans
7. **Quick Win #5** (45 min) - Builder pattern (breaking change, needs more thought)
8. **Quick Win #6** (1 hour) - Extract channel handling

**Total time for wins 1-6:** ~2.5 hours
**Total impact:** Significantly more readable code, foundation for bigger refactors

## Measuring Success

After implementing these quick wins:

- [ ] Main `start()` method is 50+ lines shorter
- [ ] Main `run()` method in TheaterRuntime is 100+ lines shorter
- [ ] At least 3 new helper methods with unit tests
- [ ] No regressions in existing test suite
- [ ] Team agrees code is more readable

## Next Steps

Once you've done these quick wins, you'll have:
1. Cleaner code that's easier to work with
2. Better understanding of the codebase
3. Foundation for the bigger state machine refactor
4. Confidence that incremental improvements work

Then you can decide: continue with quick wins, or start the state machine refactor from the migration guide!
