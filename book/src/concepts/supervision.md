# Supervision System

Theater's supervision system is one of its most powerful features, providing a robust framework for managing actor lifecycles and handling failures. Inspired by Erlang/OTP's approach to fault tolerance, the supervision system enables the creation of self-healing applications.

## Core Principles

Theater's supervision model is built on several key principles:

1. **Hierarchical Structure**: Actors are organized in a parent-child hierarchy.
2. **Failure Isolation**: When an actor fails, the failure is contained and doesn't cascade through the system.
3. **Declarative Recovery**: Parent actors define how to handle failures in their children.
4. **Let It Crash**: Rather than defensive programming, actors focus on their core functionality and let supervisors handle failures.

This approach simplifies error handling in complex systems and creates more resilient applications.

## Supervision Hierarchy

In Theater, actors form a tree-like structure:

- The **root actor** serves as the entry point and top-level supervisor
- **Parent actors** can spawn and supervise child actors
- **Child actors** report their status to their parents and can be restarted if they fail

This hierarchical approach creates clear lines of responsibility and enables targeted error handling.

## Supervisor Capabilities

Parent actors in Theater can:

1. **Spawn children**: Create new actors with specific configurations
2. **Monitor status**: Receive notifications about child state changes
3. **Restart children**: Automatically restart failed actors
4. **Stop children**: Terminate actors when they're no longer needed
5. **Access child state**: Review the state and event history of children

These capabilities give supervisors everything they need to manage their children effectively.

## Supervision Strategies

Theater supports different strategies for handling child failures:

### One-for-One Strategy

When a child actor fails, only that specific actor is restarted. This strategy is appropriate when child failures are independent of each other.

```rust
// Example of one-for-one supervision
fn handle_child_failure(&self, child_id: &ActorId, error: &Error) -> SupervisorAction {
    // Only restart the specific child that failed
    SupervisorAction::Restart(child_id.clone())
}
```

### All-for-One Strategy

When any child fails, all children are restarted. This strategy is useful when children have dependencies on each other and must be in a consistent state.

```rust
// Example of all-for-one supervision
fn handle_child_failure(&self, child_id: &ActorId, error: &Error) -> SupervisorAction {
    // Restart all children when any one fails
    SupervisorAction::RestartAll
}
```

### Temporary vs. Permanent Failures

Supervisors can also distinguish between temporary failures (which should be retried) and permanent failures (which require intervention):

```rust
fn handle_child_failure(&self, child_id: &ActorId, error: &Error) -> SupervisorAction {
    match error {
        // Temporary errors can be retried
        Error::Temporary(_) => SupervisorAction::Restart(child_id.clone()),
        
        // Permanent errors require stopping the actor
        Error::Permanent(_) => {
            log::error!("Permanent failure in child {}: {}", child_id, error);
            SupervisorAction::Stop(child_id.clone())
        }
    }
}
```

## Implementing a Supervisor Actor

To create a supervisor actor in Theater, you need to:

1. Define the actor with the supervisor interface
2. Implement the supervisor callbacks
3. Create a manifest that includes the supervisor handler

### Step 1: Define the Actor

```rust
use theater_bindgen::prelude::*;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Default)]
pub struct Supervisor {
    // Track children and their configurations
    children: HashMap<String, ChildConfig>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }
}
```

### Step 2: Implement the Supervisor Interface

```rust
impl supervisor::Supervisor for Supervisor {
    // Called when a child actor is spawned
    fn on_child_spawned(&mut self, child_id: String, config: String) -> Result<(), String> {
        let child_config: ChildConfig = serde_json::from_str(&config)
            .map_err(|e| format!("Failed to parse child config: {}", e))?;
        
        self.children.insert(child_id, child_config);
        Ok(())
    }
    
    // Called when a child's status changes
    fn on_child_status_changed(&mut self, child_id: String, status: ActorStatus) -> Result<(), String> {
        log::info!("Child {} status changed to {:?}", child_id, status);
        
        // Implement auto-restart logic if needed
        if status == ActorStatus::Failed {
            return self.restart_child(child_id);
        }
        
        Ok(())
    }
    
    // Called when a child is stopped
    fn on_child_stopped(&mut self, child_id: String) -> Result<(), String> {
        self.children.remove(&child_id);
        Ok(())
    }
    
    // Spawn a new child actor
    fn spawn_child(&mut self, manifest_path: String) -> Result<String, String> {
        // This will be handled by the Theater runtime
        // The function is implemented by the supervisor handler
        Ok("".to_string()) // Will be populated by the runtime
    }
}

theater_bindgen::export!(Supervisor);
```

### Step 3: Create the Manifest

```toml
# supervisor.toml
name = "my-supervisor"
component_path = "supervisor.wasm"

[[handlers]]
type = "supervisor"
config = {}

[[handlers]]
type = "message-server"
config = { port = 8080 }
```

## Supervision in Action

Here's a complete example of a supervision system in action:

```rust
// File: supervisor.rs
use theater_bindgen::prelude::*;

#[derive(Serialize, Deserialize, Default)]
pub struct WorkerSupervisor {
    worker_count: usize,
    workers: HashMap<String, WorkerStatus>,
}

#[derive(Serialize, Deserialize)]
enum WorkerStatus {
    Running,
    Failed,
    Stopped,
}

impl WorkerSupervisor {
    pub fn new() -> Self {
        Self::default()
    }
    
    // Initialize workers when the supervisor starts
    pub fn initialize(&mut self) -> Result<(), String> {
        // Spawn initial workers
        for i in 0..3 {
            let worker_id = self.spawn_worker()?;
            log::info!("Spawned worker {}: {}", i, worker_id);
        }
        
        self.worker_count = 3;
        Ok(())
    }
    
    fn spawn_worker(&mut self) -> Result<String, String> {
        // Path to the worker manifest
        let manifest_path = "worker.toml";
        
        // Spawn the worker through the supervisor interface
        let worker_id = supervisor::spawn_child(manifest_path)?;
        
        // Track the worker
        self.workers.insert(worker_id.clone(), WorkerStatus::Running);
        
        Ok(worker_id)
    }
}

impl supervisor::Supervisor for WorkerSupervisor {
    fn on_child_status_changed(&mut self, child_id: String, status: ActorStatus) -> Result<(), String> {
        match status {
            ActorStatus::Running => {
                self.workers.insert(child_id, WorkerStatus::Running);
            }
            ActorStatus::Failed => {
                log::warn!("Worker {} failed, restarting", child_id);
                self.workers.insert(child_id.clone(), WorkerStatus::Failed);
                
                // Restart the failed worker
                supervisor::restart_child(&child_id)?;
            }
            ActorStatus::Stopped => {
                self.workers.insert(child_id, WorkerStatus::Stopped);
            }
        }
        
        Ok(())
    }
    
    // Implement other required methods...
}

impl message_server::MessageServer for WorkerSupervisor {
    fn handle_message(&mut self, message: String) -> Result<String, String> {
        match message.as_str() {
            "initialize" => {
                self.initialize()?;
                Ok("Initialized 3 workers".to_string())
            }
            "status" => {
                let status = format!("Supervising {} workers", self.worker_count);
                Ok(status)
            }
            "add_worker" => {
                let worker_id = self.spawn_worker()?;
                self.worker_count += 1;
                Ok(format!("Added worker: {}", worker_id))
            }
            _ => Err("Unknown command".to_string()),
        }
    }
}

theater_bindgen::export!(WorkerSupervisor);
```

## Best Practices for Supervision

When implementing supervision in Theater, consider these best practices:

1. **Single Responsibility**: Each actor should have a focused responsibility to make supervision more effective
2. **Clean Restart**: Ensure actors can be restarted cleanly without side effects
3. **Stateless When Possible**: Stateless actors are easier to restart and recover
4. **Monitor Carefully**: Use the monitoring features to track actor behavior over time
5. **Graceful Degradation**: Design systems to work in a degraded state if some components fail
6. **Clear Boundaries**: Define clear interfaces between actors to minimize dependencies

## Comparing to Other Supervision Models

Theater's supervision system draws inspiration from Erlang/OTP but adapts it to the WebAssembly context:

| Feature | Erlang/OTP | Theater |
|---------|------------|---------|
| Granularity | Process-based | Actor-based |
| State Management | Transient | Persistent with history |
| Recovery Strategies | One-for-one, Rest-for-one, All-for-one | One-for-one, All-for-one |
| Linking | Bidirectional process links | Parent-child hierarchy |
| Error Propagation | Signal-based | Event-based |

The WebAssembly foundation adds important capabilities like deterministic execution and sandboxing, which complement the supervision model.

## Real-World Applications

Theater's supervision system is particularly valuable in several scenarios:

1. **Long-running services** that need to maintain availability even when components fail
2. **AI-generated code** where robustness can't be guaranteed through traditional means
3. **Multi-tenant systems** where failures in one tenant shouldn't affect others
4. **Edge computing** where systems must be self-healing due to limited human intervention

In the following chapters, we'll explore how to apply supervision patterns to specific use cases and how to integrate supervision with other Theater features like state traceability.
