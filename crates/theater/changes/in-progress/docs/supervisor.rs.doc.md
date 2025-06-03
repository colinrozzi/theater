
# Supervisor Host Module Documentation

## SupervisorHost Struct

```rust
/// # SupervisorHost
///
/// A host implementation that provides actor supervision capabilities to WebAssembly actors.
///
/// ## Purpose
///
/// The SupervisorHost enables hierarchical actor supervision within the Theater system,
/// implementing an Erlang-style supervision model. It allows parent actors to manage their
/// child actors by spawning, stopping, restarting, and monitoring them. This creates a
/// robust foundation for building fault-tolerant applications with clear hierarchies
/// of responsibility.
///
/// This implementation serves as the bridge between the WebAssembly interface defined in
/// `supervisor.wit` and the actual actor management operations in the Theater runtime.
///
/// ## Example
///
/// ```rust
/// use theater::host::supervisor::SupervisorHost;
/// use theater::config::SupervisorHostConfig;
/// use theater::actor_handle::ActorHandle;
/// use theater::shutdown::ShutdownReceiver;
/// use theater::wasm::ActorComponent;
/// 
/// async fn example() -> anyhow::Result<()> {
///     // Create a supervisor host
///     let config = SupervisorHostConfig {};
///     let supervisor = SupervisorHost::new(config);
///     
///     // Set up host functions for an actor component
///     let mut actor_component = ActorComponent::dummy(); // For demonstration
///     supervisor.setup_host_functions(&mut actor_component).await?;
///     
///     // Start the supervisor host
///     let actor_handle = ActorHandle::dummy(); // For demonstration
///     let shutdown_receiver = ShutdownReceiver::dummy(); // For demonstration
///     supervisor.start(actor_handle, shutdown_receiver).await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Security
///
/// The SupervisorHost implements security controls for actor supervision:
///
/// 1. **Hierarchical Isolation**: Each actor can only supervise its own children,
///    enforcing a strict parent-child hierarchy that prevents unauthorized access
///    to other actors in the system.
///
/// 2. **Event Tracking**: All supervision operations are recorded in the event chain,
///    providing a comprehensive audit trail of actor lifecycle management.
///
/// 3. **Safe State Access**: Parent actors can access their children's state but not
///    the state of other actors in the system, maintaining appropriate isolation.
///
/// ## Implementation Notes
///
/// The SupervisorHost translates between the WebAssembly interface and the Theater
/// runtime's actor management operations. It uses message passing through the Theater
/// command channel to communicate with the runtime, ensuring proper synchronization
/// and fault isolation.
///
/// Each supervision operation is wrapped in a function that validates the request,
/// sends the appropriate command to the runtime, waits for the response, records
/// the event, and translates the result back to the WebAssembly interface.
pub struct SupervisorHost {}
```

## SupervisorError Enum

```rust
/// # SupervisorError
///
/// Error types specific to supervisor operations.
///
/// ## Purpose
///
/// This error enum defines the various types of errors that can occur during
/// supervision operations, providing detailed context for troubleshooting.
///
/// ## Example
///
/// ```rust
/// use theater::host::supervisor::SupervisorError;
/// 
/// fn example() {
///     // Create a handler error
///     let handler_error = SupervisorError::HandlerError(
///         "Child actor not found".to_string()
///     );
///     
///     // Handle the error
///     match handler_error {
///         SupervisorError::HandlerError(msg) => println!("Handler error: {}", msg),
///         SupervisorError::ActorError(e) => println!("Actor error: {}", e),
///         _ => println!("Other error"),
///     }
/// }
/// ```
///
/// ## Implementation Notes
///
/// This enum derives from `thiserror::Error` to provide consistent error handling
/// and formatting. It includes error variants for supervision-specific issues and
/// implements conversions from common error types to simplify error propagation.
#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}
```

## Core Methods

```rust
impl SupervisorHost {
    /// # New
    ///
    /// Creates a new SupervisorHost with the specified configuration.
    ///
    /// ## Purpose
    ///
    /// This constructor initializes a new SupervisorHost instance. The SupervisorHost
    /// currently has minimal configuration needs, but the constructor follows the
    /// standard pattern of accepting a config struct for future extensibility.
    ///
    /// ## Parameters
    ///
    /// * `_config` - A `SupervisorHostConfig` struct (currently empty)
    ///
    /// ## Returns
    ///
    /// A new `SupervisorHost` instance
    ///
    /// ## Implementation Notes
    ///
    /// The current implementation takes a configuration parameter for consistency
    /// with other handlers, but doesn't currently use it since supervisor functionality
    /// doesn't require specific configuration. This may change in future versions.
    pub fn new(_config: SupervisorHostConfig) -> Self {
        // Method implementation...
    }

    /// # Setup Host Functions
    ///
    /// Configures the WebAssembly component with supervisor host functions.
    ///
    /// ## Purpose
    ///
    /// This method registers all the supervision-related host functions that actors
    /// can call through the WebAssembly component model interface. It creates a bridge
    /// between the WebAssembly interface defined in `supervisor.wit` and the actual
    /// actor management operations in the Theater runtime.
    ///
    /// ## Parameters
    ///
    /// * `actor_component` - A mutable reference to the ActorComponent being configured
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Success or an error if host functions could not be set up
    ///
    /// ## Security
    ///
    /// All supervision functions enforce the hierarchical isolation model:
    /// - Actors can only interact with their own children
    /// - All operations are recorded in the event chain for audit
    /// - Input validation ensures proper actor ID formats
    ///
    /// ## Implementation Notes
    ///
    /// This method sets up several host functions in the theater:simple/supervisor namespace:
    /// - spawn: Create a new child actor from a manifest
    /// - resume: Resume a previously stopped actor with its state
    /// - list-children: Get a list of all child actor IDs
    /// - restart-child: Restart a specific child actor
    /// - stop-child: Stop a specific child actor
    /// - get-child-state: Retrieve the current state of a child actor
    /// - get-child-events: Retrieve the event history of a child actor
    ///
    /// Each function is wrapped to handle validation, error handling, and event recording.
    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        // Method implementation...
    }

    /// # Add Export Functions
    ///
    /// Adds actor export functions for supervision operations.
    ///
    /// ## Purpose
    ///
    /// This method would register functions that the host can call on the actor's exports
    /// for supervision-related callbacks. However, the SupervisorHost currently doesn't
    /// require any callbacks to the actor, so this method is a no-op.
    ///
    /// ## Parameters
    ///
    /// * `_actor_instance` - A mutable reference to the ActorInstance
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Always succeeds as there are no export functions to add
    ///
    /// ## Implementation Notes
    ///
    /// This is a placeholder for potential future expansion. Currently, supervision
    /// operations are one-way (actor calls host) with no need for callbacks.
    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        // Method implementation...
    }

    /// # Start
    ///
    /// Starts the SupervisorHost handler.
    ///
    /// ## Purpose
    ///
    /// This method initializes the SupervisorHost and prepares it for handling supervision
    /// operations from the actor. It logs the startup information and completes
    /// any necessary initialization.
    ///
    /// ## Parameters
    ///
    /// * `_actor_handle` - A handle to the actor that this handler is associated with
    /// * `_shutdown_receiver` - A receiver for shutdown signals to cleanly terminate
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Success or an error if the handler could not be started
    ///
    /// ## Implementation Notes
    ///
    /// The SupervisorHost doesn't currently require background tasks, so this method
    /// primarily logs the startup information and returns success. The actual handler
    /// functionality is event-driven through the host functions.
    pub async fn start(
        &self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        // Method implementation...
    }
}
```

## Registered Host Functions

### Spawn Function

```rust
/// # Spawn
///
/// Spawns a new child actor from a manifest.
///
/// ## Purpose
///
/// This host function allows parent actors to create new child actors, establishing
/// a supervision relationship. The parent can provide optional initialization data
/// to the new actor during creation.
///
/// ## Parameters
///
/// * `manifest` - Path to the actor manifest file
/// * `init_bytes` - Optional initialization data for the new actor
///
/// ## Returns
///
/// `Result<String, String>` - The new actor's ID on success, or an error message
///
/// ## Security
///
/// This function records the spawn operation in the event chain for audit purposes.
/// The parent-child relationship is tracked within the Theater runtime to enforce
/// hierarchical isolation.
///
/// ## Implementation Notes
///
/// The implementation sends a SpawnActor command to the Theater runtime, which handles
/// the actual actor creation. The parent's ID is included in the command to establish
/// the supervision relationship.
pub fn spawn(
    ctx: StoreContextMut<'_, ActorStore>,
    (manifest, init_bytes): (String, Option<Vec<u8>>)
) -> Future<Result<(Result<String, String>,)>> {
    // Function implementation...
}
```

### Resume Function

```rust
/// # Resume
///
/// Resumes a previously stopped actor with a provided state.
///
/// ## Purpose
///
/// This host function allows parent actors to resume a previously stopped actor
/// with a specific state. This is useful for implementing migration, recovery,
/// or snapshot-based patterns where actor state needs to be preserved.
///
/// ## Parameters
///
/// * `manifest` - Path to the actor manifest file
/// * `state_bytes` - Optional state data to initialize the actor with
///
/// ## Returns
///
/// `Result<String, String>` - The resumed actor's ID on success, or an error message
///
/// ## Security
///
/// This function records the resume operation in the event chain for audit purposes.
/// The parent-child relationship is tracked to enforce hierarchical isolation.
///
/// ## Implementation Notes
///
/// The implementation sends a ResumeActor command to the Theater runtime, which
/// handles the actual actor resumption. The parent's ID is included in the command
/// to establish the supervision relationship.
pub fn resume(
    ctx: StoreContextMut<'_, ActorStore>,
    (manifest, state_bytes): (String, Option<Vec<u8>>)
) -> Future<Result<(Result<String, String>,)>> {
    // Function implementation...
}
```

### List Children Function

```rust
/// # List Children
///
/// Lists all child actors of the current actor.
///
/// ## Purpose
///
/// This host function allows parent actors to retrieve a list of all their child
/// actor IDs. This is useful for monitoring and managing the supervision tree.
///
/// ## Parameters
///
/// None
///
/// ## Returns
///
/// `Vec<String>` - A list of child actor IDs
///
/// ## Security
///
/// This function only returns children directly supervised by the calling actor,
/// enforcing hierarchical isolation. The operation is recorded in the event chain
/// for audit purposes.
///
/// ## Implementation Notes
///
/// The implementation sends a ListChildren command to the Theater runtime, which
/// returns the IDs of all actors that have the calling actor as their parent.
pub fn list_children(
    ctx: StoreContextMut<'_, ActorStore>,
    ()
) -> Future<Result<(Vec<String>,)>> {
    // Function implementation...
}
```

### Restart Child Function

```rust
/// # Restart Child
///
/// Restarts a specific child actor.
///
/// ## Purpose
///
/// This host function allows parent actors to restart one of their child actors,
/// which is useful for error recovery and fault tolerance. The child actor will
/// be stopped and then started again with its last known state.
///
/// ## Parameters
///
/// * `child_id` - The ID of the child actor to restart
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message
///
/// ## Security
///
/// This function validates that the target actor is actually a child of the calling
/// actor, enforcing hierarchical isolation. The operation is recorded in the event
/// chain for audit purposes.
///
/// ## Implementation Notes
///
/// The implementation sends a RestartActor command to the Theater runtime after
/// validating the child actor ID. The restart operation is performed asynchronously,
/// and the result is returned to the caller.
pub fn restart_child(
    ctx: StoreContextMut<'_, ActorStore>,
    (child_id,): (String,)
) -> Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```

### Stop Child Function

```rust
/// # Stop Child
///
/// Stops a specific child actor.
///
/// ## Purpose
///
/// This host function allows parent actors to stop one of their child actors.
/// This is useful for resource management, cleanup, and graceful shutdown
/// of parts of the actor hierarchy.
///
/// ## Parameters
///
/// * `child_id` - The ID of the child actor to stop
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message
///
/// ## Security
///
/// This function validates that the target actor is actually a child of the calling
/// actor, enforcing hierarchical isolation. The operation is recorded in the event
/// chain for audit purposes.
///
/// ## Implementation Notes
///
/// The implementation sends a StopActor command to the Theater runtime after
/// validating the child actor ID. The stop operation is performed asynchronously,
/// and the result is returned to the caller.
pub fn stop_child(
    ctx: StoreContextMut<'_, ActorStore>,
    (child_id,): (String,)
) -> Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```

### Get Child State Function

```rust
/// # Get Child State
///
/// Retrieves the current state of a child actor.
///
/// ## Purpose
///
/// This host function allows parent actors to access the current state of one of their
/// child actors. This is useful for monitoring, coordination, and implementing
/// supervision strategies that depend on child actor state.
///
/// ## Parameters
///
/// * `child_id` - The ID of the child actor whose state to retrieve
///
/// ## Returns
///
/// `Result<Option<Vec<u8>>, String>` - The child actor's state (if any) or an error message
///
/// ## Security
///
/// This function validates that the target actor is actually a child of the calling
/// actor, enforcing hierarchical isolation. The operation is recorded in the event
/// chain for audit purposes.
///
/// ## Implementation Notes
///
/// The implementation sends a GetActorState command to the Theater runtime after
/// validating the child actor ID. The state retrieval is performed asynchronously,
/// and the result is returned to the caller. If the child has no state, None is returned.
pub fn get_child_state(
    ctx: StoreContextMut<'_, ActorStore>,
    (child_id,): (String,)
) -> Future<Result<(Result<Option<Vec<u8>>, String>,)>> {
    // Function implementation...
}
```

### Get Child Events Function

```rust
/// # Get Child Events
///
/// Retrieves the event history of a child actor.
///
/// ## Purpose
///
/// This host function allows parent actors to access the event history of one of their
/// child actors. This is useful for debugging, auditing, and implementing supervision
/// strategies that depend on understanding child actor behavior.
///
/// ## Parameters
///
/// * `child_id` - The ID of the child actor whose events to retrieve
///
/// ## Returns
///
/// `Result<Vec<ChainEvent>, String>` - The child actor's events or an error message
///
/// ## Security
///
/// This function validates that the target actor is actually a child of the calling
/// actor, enforcing hierarchical isolation. The operation is recorded in the event
/// chain for audit purposes.
///
/// ## Implementation Notes
///
/// The implementation sends a GetActorEvents command to the Theater runtime after
/// validating the child actor ID. The events retrieval is performed asynchronously,
/// and the result is returned to the caller. The events are returned in chronological
/// order.
pub fn get_child_events(
    ctx: StoreContextMut<'_, ActorStore>,
    (child_id,): (String,)
) -> Future<Result<(Result<Vec<ChainEvent>, String>,)>> {
    // Function implementation...
}
```

## Helper Types

```rust
/// # SupervisorEvent
///
/// A structure for representing supervisor-related events.
///
/// ## Purpose
///
/// This structure provides a standardized format for representing supervisor events
/// that can be serialized and passed between actors. It includes fields for the
/// event type, affected actor ID, and optional event data.
///
/// ## Implementation Notes
///
/// This type is primarily used internally within the supervisor implementation to
/// handle event serialization and deserialization. It derives from Serialize and
/// Deserialize to enable serialization support.
#[derive(Debug, Serialize, Deserialize)]
struct SupervisorEvent {
    event_type: String,
    actor_id: String,
    data: Option<Vec<u8>>,
}
```
