# Change Request: Actor Shutdown System Implementation

## Overview

This change request outlines the implementation of a robust shutdown mechanism for the Theater actor system. The primary goal is to ensure that all processes associated with an actor are properly terminated when the actor is shut down, with particular attention to complex handlers like `http_framework`.

## Scope

- Create a broadcast shutdown channel system
- Enable per-actor shutdown signaling
- Ensure all handler processes receive the shutdown signal
- Implement graceful shutdown with timeout fallback
- Focus only on process termination (not state persistence or other cleanup)

## Out of Scope

- State preservation during shutdown
- Detailed cleanup of actor resources beyond process termination
- Modifications to the supervision system
- Changes to the actor API

## Implementation Details

### 1. Create Shutdown Types

**File: `src/shutdown.rs`**

```rust
use tokio::sync::broadcast;
use std::time::Duration;

pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct ShutdownSignal {}

pub struct ShutdownController {
    sender: broadcast::Sender<ShutdownSignal>,
}

impl ShutdownController {
    pub fn new() -> (Self, ShutdownReceiver) {
        let (sender, receiver) = broadcast::channel(8);
        (
            Self { sender },
            ShutdownReceiver { receiver },
        )
    }
    
    pub fn subscribe(&self) -> ShutdownReceiver {
        ShutdownReceiver {
            receiver: self.sender.subscribe(),
        }
    }
    
    pub fn signal_shutdown(&self) {
        debug!("Broadcasting shutdown signal to {} receivers", self.sender.receiver_count());
        let send_count = self.sender.send(ShutdownSignal {}).unwrap_or(0);
        debug!("Shutdown signal sent to {} receivers", send_count);
    }
}

pub struct ShutdownReceiver {
    receiver: broadcast::Receiver<ShutdownSignal>,
}

impl ShutdownReceiver {
    pub async fn wait_for_shutdown(&mut self) -> ShutdownSignal {
        debug!("Waiting for shutdown signal");
        match self.receiver.recv().await {
            Ok(signal) => {
                debug!("Received shutdown signal");
                signal
            },
            Err(e) => {
                debug!("Shutdown channel error: {}, using default signal", e);
                ShutdownSignal {} // Default if channel closed
            }
        }
    }
}
```

### 2. Update ActorProcess to include ShutdownController

**File: `src/theater_runtime.rs`**

```rust
// Add to imports
use crate::shutdown::{ShutdownController, DEFAULT_SHUTDOWN_TIMEOUT};

// Modify ActorProcess struct
pub struct ActorProcess {
    pub actor_id: TheaterId,
    pub process: JoinHandle<ActorRuntime>,
    pub mailbox_tx: mpsc::Sender<ActorMessage>,
    pub operation_tx: mpsc::Sender<ActorOperation>,
    pub children: HashSet<TheaterId>,
    pub status: ActorStatus,
    pub manifest_path: String,
    pub shutdown_controller: ShutdownController, // Added field
}
```

### 3. Update TheaterRuntime's spawn_actor method

**File: `src/theater_runtime.rs`**

```rust
async fn spawn_actor(
    &mut self,
    manifest_path: String,
    init_bytes: Option<Vec<u8>>,
    parent_id: Option<TheaterId>,
    init: bool,
) -> Result<TheaterId> {
    // Existing code...
    
    // Create a shutdown controller for this specific actor
    let (shutdown_controller, shutdown_receiver) = ShutdownController::new();
    
    let actor_operation_tx = operation_tx.clone();
    let actor_runtime_process = tokio::spawn(async move {
        let actor_id = TheaterId::generate();
        debug!("Initializing actor runtime");
        debug!("Starting actor runtime");
        response_tx.send(actor_id.clone()).unwrap();
        ActorRuntime::start(
            actor_id,
            &manifest,
            init_bytes,
            theater_tx,
            mailbox_rx,
            operation_rx,
            actor_operation_tx,
            init,
            shutdown_receiver, // New parameter
        )
        .await
        .unwrap()
    });

    match response_rx.await {
        Ok(actor_id) => {
            // Existing code...
            
            let process = ActorProcess {
                actor_id: actor_id.clone(),
                process: actor_runtime_process,
                mailbox_tx,
                operation_tx,
                children: HashSet::new(),
                status: ActorStatus::Running,
                manifest_path: manifest_path.clone(),
                shutdown_controller, // Store the controller
            };
            
            // Rest of existing code...
        }
        // ...
    }
}
```

### 4. Update stop_actor method

**File: `src/theater_runtime.rs`**

```rust
async fn stop_actor(&mut self, actor_id: TheaterId) -> Result<()> {
    debug!("Stopping actor: {:?}", actor_id);
    
    if let Some(proc) = self.actors.get(&actor_id) {
        debug!("Actor {:?} has {} children to stop first", actor_id, proc.children.len());
        
        // First, stop all children recursively
        let children = proc.children.clone();
        for (index, child_id) in children.iter().enumerate() {
            debug!("Stopping child {}/{} with ID {:?} of parent {:?}", 
                   index + 1, children.len(), child_id, actor_id);
            Box::pin(self.stop_actor(child_id.clone())).await?;
            debug!("Successfully stopped child {:?}", child_id);
        }
        
        // Signal this specific actor to shutdown
        debug!("Sending shutdown signal to actor {:?}", actor_id);
        proc.shutdown_controller.signal_shutdown();
        debug!("Shutdown signal sent to actor {:?}, waiting for grace period", actor_id);
        
        // Allow some time for graceful shutdown
        tokio::time::sleep(DEFAULT_SHUTDOWN_TIMEOUT).await;
        debug!("Grace period for actor {:?} complete", actor_id);
        
        // Force abort if still running
        if let Some(proc) = self.actors.get(&actor_id) {
            debug!("Force aborting actor {:?} task after grace period", actor_id);
            proc.process.abort();
            debug!("Actor {:?} task aborted", actor_id);
        }
        
        // Remove from actors map
        if let Some(mut removed_proc) = self.actors.remove(&actor_id) {
            removed_proc.status = ActorStatus::Stopped;
            debug!("Actor {:?} stopped and removed from runtime", actor_id);
        }
    } else {
        warn!("Attempted to stop non-existent actor: {:?}", actor_id);
    }
    
    Ok(())
}
```

### 5. Update ActorRuntime to Receive Shutdown Signal

**File: `src/actor_runtime.rs`**

```rust
// Add to imports
use crate::shutdown::{ShutdownController, ShutdownReceiver};

pub struct ActorRuntime {
    pub actor_id: TheaterId,
    handler_tasks: Vec<JoinHandle<()>>,
    actor_executor_task: JoinHandle<()>,
    shutdown_controller: ShutdownController, // New field
}

impl ActorRuntime {
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        state_bytes: Option<Vec<u8>>,
        theater_tx: Sender<TheaterCommand>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        init: bool,
        parent_shutdown_receiver: ShutdownReceiver, // New parameter
    ) -> Result<Self> {
        // Create a local shutdown controller for this runtime
        let (shutdown_controller, _) = ShutdownController::new();
        
        // Set up handlers as before...
        
        // For each handler, pass a shutdown receiver
        for mut handler in &handlers {
            let handler_shutdown = shutdown_controller.subscribe();
            // ... add to handler context
        }
        
        // Set up executor with shutdown
        let executor_shutdown = shutdown_controller.subscribe();
        let mut actor_executor = ActorExecutor::new(
            actor_instance, 
            operation_rx,
            executor_shutdown
        );
        
        // Start executor
        let executor_task = tokio::spawn(async move {
            actor_executor.run().await;
        });
        
        // Start handlers with shutdown receivers
        let mut handler_tasks = Vec::new();
        for mut handler in handlers {
            let handler_shutdown = shutdown_controller.subscribe();
            let actor_handle = actor_handle.clone();
            
            let handler_task = tokio::spawn(async move {
                if let Err(e) = handler.start(actor_handle, handler_shutdown).await {
                    warn!("Handler failed: {:?}", e);
                }
            });
            
            handler_tasks.push(handler_task);
        }
        
        // Monitor parent shutdown signal and propagate
        let shutdown_controller_clone = shutdown_controller.clone();
        let actor_id_clone = id.clone();
        tokio::spawn(async move {
            debug!("Actor {:?} waiting for parent shutdown signal", actor_id_clone);
            parent_shutdown_receiver.wait_for_shutdown().await;
            info!("Actor {:?} runtime received parent shutdown signal", actor_id_clone);
            debug!("Propagating shutdown signal to all handler components for actor {:?}", actor_id_clone);
            shutdown_controller_clone.signal_shutdown();
            debug!("Shutdown signal propagated to all components of actor {:?}", actor_id_clone);
        });
        
        Ok(ActorRuntime {
            actor_id: id.clone(),
            handler_tasks,
            actor_executor_task: executor_task,
            shutdown_controller,
        })
    }
}
```

### 6. Update ActorExecutor to Handle Shutdown Signal

**File: `src/actor_executor.rs`**

```rust
// Add to imports
use crate::shutdown::ShutdownReceiver;

pub struct ActorExecutor {
    actor_instance: ActorInstance,
    operation_rx: mpsc::Receiver<ActorOperation>,
    metrics: MetricsCollector,
    shutdown_receiver: ShutdownReceiver, // New field
    shutdown_initiated: bool,
}

impl ActorExecutor {
    pub fn new(
        actor_instance: ActorInstance,
        operation_rx: mpsc::Receiver<ActorOperation>,
        shutdown_receiver: ShutdownReceiver, // New parameter
    ) -> Self {
        Self {
            actor_instance,
            operation_rx,
            metrics: MetricsCollector::new(),
            shutdown_receiver,
            shutdown_initiated: false,
        }
    }
    
    pub async fn run(&mut self) {
        info!("Actor executor starting");

        loop {
            tokio::select! {
                // Monitor shutdown channel
                signal = self.shutdown_receiver.wait_for_shutdown() => {
                    info!("Actor executor received shutdown signal");
                    debug!("Executor for actor instance starting shutdown sequence");
                    self.shutdown_initiated = true;
                    debug!("Executor marked as shutting down, will reject new operations");
                    break;
                }
                
                // Handle operations as before
                Some(op) = self.operation_rx.recv() => {
                    if self.shutdown_initiated {
                        // Reject operations during shutdown
                        continue;
                    }
                    
                    // Existing operation handling...
                }
                
                else => {
                    info!("Operation channel closed, shutting down");
                    break;
                }
            }
        }

        info!("Actor executor shutting down");
        self.cleanup().await;
    }
    
    // No changes to other methods
}
```

### 7. Update Handler Interface

**File: `src/host/handler.rs`**

```rust
// Add to imports
use crate::shutdown::ShutdownReceiver;

impl Handler {
    pub async fn start(&mut self, actor_handle: ActorHandle, shutdown_receiver: ShutdownReceiver) -> Result<()> {
        match self {
            Handler::MessageServer(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting message server")),
            Handler::HttpServer(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting http server")),
            // ... other handlers similarly updated
        }
    }
    
    // Existing methods unchanged
}
```

### 8. Update HTTP Server Handler

**File: `src/host/http_server.rs`**

```rust
// Add to imports
use crate::shutdown::ShutdownReceiver;

impl HttpServerHost {
    // Other methods unchanged...
    
    pub async fn start(&self, actor_handle: ActorHandle, mut shutdown_receiver: ShutdownReceiver) -> Result<()> {
        let app = Router::new()
            // ... router setup
            .with_state(Arc::new(actor_handle.clone()));
            
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Starting http server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Listening on {}", addr);
        
        // Start with graceful shutdown
        let server = axum::serve(listener, app.into_make_service());
        
        // Use with_graceful_shutdown
        let server_task = server.with_graceful_shutdown(async move {
            debug!("HTTP server on port {} waiting for shutdown signal", self.port);
            shutdown_receiver.wait_for_shutdown().await;
            info!("HTTP server on port {} received shutdown signal", self.port);
            debug!("Beginning graceful shutdown of HTTP server on port {}", self.port);
        });
        
        server_task.await?;
        info!("HTTP server on port {} shut down gracefully", self.port);
        debug!("HTTP server resources for port {} released", self.port);
        Ok(())
    }
}
```

### 9. Update HTTP Framework Handler

**File: `src/host/framework/mod.rs`**

```rust
// Add to imports
use crate::shutdown::ShutdownReceiver;

pub struct HttpFramework {
    // Add field to track servers
    servers: RwLock<HashMap<u64, ServerHandle>>,
}

// Define a type to track server handles
struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl HttpFramework {
    // Existing methods...
    
    pub async fn start(&mut self, actor_handle: ActorHandle, mut shutdown_receiver: ShutdownReceiver) -> Result<()> {
        // Create task to monitor shutdown signal
        let servers_ref = self.servers.clone();
        
        tokio::spawn(async move {
            debug!("HTTP Framework shutdown monitor started");
            
            // Wait for shutdown signal
            shutdown_receiver.wait_for_shutdown().await;
            info!("HTTP Framework received shutdown signal");
            
            // Shut down all servers
            let servers = servers_ref.read().await;
            debug!("HTTP Framework shutting down {} servers", servers.len());
            
            for (id, handle) in servers.iter() {
                debug!("Initiating shutdown of HTTP Framework server {}", id);
                
                if let Some(tx) = &handle.shutdown_tx {
                    debug!("Sending graceful shutdown signal to server {}", id);
                    if let Err(e) = tx.send(()) {
                        warn!("Failed to send shutdown to HTTP server {}: {}", id, e);
                    } else {
                        debug!("Shutdown signal sent to server {}", id);
                    }
                } else {
                    debug!("No shutdown channel for server {}", id);
                }
                
                // Give a moment for graceful shutdown
                debug!("Waiting for server {} to shut down gracefully", id);
                tokio::time::sleep(Duration::from_millis(100)).await;
                
                // Force abort if still running
                if let Some(task) = &handle.task {
                    if !task.is_finished() {
                        debug!("Server {} still running after grace period, aborting", id);
                        task.abort();
                        info!("Forcibly aborted HTTP Framework server {}", id);
                    } else {
                        debug!("Server {} shutdown gracefully", id);
                    }
                } else {
                    debug!("No task handle for server {}", id);
                }
            }
            
            debug!("HTTP Framework shutdown complete");
        });
        
        Ok(())
    }
    
    // Modify server creation to save shutdown handle
    async fn create_server_impl(&self, config: ServerConfig) -> Result<u64> {
        // Existing server creation code...
        
        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        
        // Start server with graceful shutdown
        let server_task = tokio::spawn(async move {
            server.with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            }).await;
        });
        
        // Save server handle
        let server_handle = ServerHandle {
            shutdown_tx: Some(shutdown_tx),
            task: Some(server_task),
        };
        
        let server_id = // generate ID
        self.servers.write().await.insert(server_id, server_handle);
        
        Ok(server_id)
    }
}
```

### 10. Update Remaining Handlers

Similar changes should be made to all other handlers:
- MessageServerHost
- WebSocketServerHost 
- FileSystemHost
- HttpClientHost
- StoreHost
- SupervisorHost

## Testing Plan

1. Test actor shutdown
   - Create an actor with HTTP and WebSocket handlers
   - Shut down the actor
   - Verify all ports are released
   - Verify no zombie processes

2. Test shutdown hierarchy
   - Create parent-child actor relationships
   - Shut down parent
   - Verify all children and their handlers shut down

3. Test Theater shutdown
   - Create multiple actors
   - Shut down Theater
   - Verify all actors and their handlers shut down

## Rollout Plan

1. Implement and test shutdown controller in isolation
2. Update ActorProcess and TheaterRuntime
3. Integrate with ActorRuntime and ActorExecutor
4. Update core handlers (MessageServer, HTTP)
5. Update all remaining handlers
6. Integrate with supervision system

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Race conditions | Ensure proper use of async/await and timeout fallbacks |
| Deadlocks | Implement timeout for all shutdown operations |
| Resource leaks | Use abort as fallback after timeout |
| Performance impact | Keep shutdown channels small with limited capacity |

## Conclusion

This change will provide a robust way to shut down actors and all their related processes, ensuring clean termination and resource release. The implementation focuses solely on process termination without modifying the actor API or state management system.

## Additional Notes

1. Debug logging has been added throughout the shutdown process to provide visibility into:
   - Shutdown signal propagation
   - Component termination sequence
   - Timeouts and fallbacks
   - Resource cleanup

2. Log levels are used as follows:
   - `debug!` - Detailed shutdown sequence steps
   - `info!` - Major shutdown transitions
   - `warn!` - Timeout or forced terminations
   - `error!` - Failures during shutdown

3. The debug logs can be enabled during testing using:
   ```bash
   RUST_LOG=theater=debug cargo run
   ```

4. For production use, consider using a more selective approach:
   ```bash
   RUST_LOG=theater=info,theater::shutdown=debug cargo run
   ```

These comprehensive logs will make it much easier to diagnose any issues with the shutdown process and ensure all resources are properly cleaned up.