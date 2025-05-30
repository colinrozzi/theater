
# Handler Module Documentation

## Handler Enum

```rust
/// # Handler
///
/// The primary enum defining all possible host-provided capabilities for WebAssembly actors in the Theater system.
///
/// ## Purpose
///
/// The Handler enum is a crucial component that encapsulates all the different services and capabilities
/// that the Theater runtime can provide to WebAssembly actors. Each variant represents a different type
/// of functionality that can be exposed to actors, such as filesystem access, HTTP communication, timing
/// services, and actor supervision.
///
/// This enum serves as a type-safe way to manage and dispatch operations to the appropriate handler implementation
/// while maintaining a unified interface for starting handlers, setting up host functions, and adding export functions.
///
/// ## Example
///
/// ```rust
/// use theater::host::{Handler, filesystem::FileSystemHost};
/// use theater::config::FileSystemHandlerConfig;
/// use theater::actor_handle::ActorHandle;
/// use theater::shutdown::ShutdownReceiver;
/// 
/// async fn example() -> anyhow::Result<()> {
///     // Create a filesystem handler
///     let config = FileSystemHandlerConfig {
///         path: Some("/tmp/actor_files".to_string()),
///         new_dir: Some(false),
///         allowed_commands: None,
///     };
///     let mut handler = Handler::FileSystem(FileSystemHost::new(config));
///     
///     // Start the handler with an actor handle and shutdown receiver
///     let actor_handle = ActorHandle::dummy(); // For demonstration
///     let shutdown_receiver = ShutdownReceiver::dummy(); // For demonstration
///     handler.start(actor_handle, shutdown_receiver).await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Security
///
/// The Handler enum plays a critical role in maintaining security boundaries between WebAssembly actors
/// and the host system. Each handler implementation is responsible for enforcing appropriate access controls,
/// validating inputs, and ensuring that actors cannot bypass sandbox restrictions.
/// 
/// Handlers should carefully audit all operations that expose host system capabilities to WebAssembly actors
/// and implement proper validation, especially for operations like filesystem access and command execution.
///
/// ## Implementation Notes
///
/// The Handler enum uses a match-based dispatch pattern to delegate operations to the appropriate concrete
/// handler implementation. When adding new handler types, ensure that all methods in the Handler impl block
/// are updated to include the new variant.
///
/// Each handler is responsible for:
/// 1. Setting up host functions that actors can call
/// 2. Implementing any export functions that the handler might call on the actor
/// 3. Managing its own lifecycle when started
/// 4. Properly cleaning up resources when shut down
pub enum Handler {
    MessageServer(MessageServerHost),
    FileSystem(FileSystemHost),
    HttpClient(HttpClientHost),
    HttpFramework(HttpFramework),
    Runtime(RuntimeHost),
    Supervisor(SupervisorHost),
    Store(StoreHost),
    Timing(TimingHost),
}
```

## Handler Methods

```rust
impl Handler {
    /// # Start
    ///
    /// Starts the handler's background operations using the provided actor handle and shutdown receiver.
    ///
    /// ## Purpose
    ///
    /// This method initiates the handler's operations, setting up any background tasks, resources,
    /// or connections that the handler needs to function properly. It uses the actor handle to
    /// communicate with the actor when needed and monitors the shutdown receiver to properly
    /// terminate when requested.
    ///
    /// ## Parameters
    ///
    /// * `actor_handle` - A handle to the actor that this handler is associated with
    /// * `shutdown_receiver` - A receiver for shutdown signals to cleanly terminate the handler
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Success or an error if the handler could not be started
    ///
    /// ## Security
    ///
    /// When starting handlers, care must be taken to ensure proper isolation between handlers
    /// serving different actors to prevent information leakage or cross-contamination.
    ///
    /// ## Implementation Notes
    ///
    /// Each handler variant has its own implementation of the start method. This method dispatches
    /// to the appropriate implementation based on the variant.
    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        // Method implementation...
    }

    /// # Setup Host Functions
    ///
    /// Configures the WebAssembly component with host functions that the actor can call.
    ///
    /// ## Purpose
    ///
    /// This method is responsible for setting up the host functions that the actor can call through
    /// the WebAssembly interface. It establishes the communication bridge between the WebAssembly
    /// component and the host system, allowing actors to request services from the host.
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
    /// Host functions are a primary security boundary in the WebAssembly sandbox model.
    /// Each handler must carefully validate all inputs from actors to prevent security
    /// issues such as path traversal, command injection, or resource exhaustion.
    ///
    /// ## Implementation Notes
    ///
    /// Host functions are set up using the WebAssembly component model's linker system.
    /// Each handler installs its own set of functions into the appropriate namespace
    /// in the component's linker.
    pub async fn setup_host_functions(&mut self, actor_component: &mut ActorComponent) -> Result<()> {
        // Method implementation...
    }

    /// # Add Export Functions
    ///
    /// Adds functions that the host can call on the actor's exports.
    ///
    /// ## Purpose
    ///
    /// This method configures the functions that the host handler can call on the actor's exports.
    /// While host functions allow the actor to call the host, export functions allow the host
    /// to call the actor, establishing bidirectional communication.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - A mutable reference to the ActorInstance being configured
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Success or an error if export functions could not be added
    ///
    /// ## Implementation Notes
    ///
    /// Not all handlers require export functions. Some handlers, such as the filesystem handler,
    /// only provide services to the actor but don't need to call back into the actor.
    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        // Method implementation...
    }

    /// # Name
    ///
    /// Returns the name of the handler as a string.
    ///
    /// ## Purpose
    ///
    /// This method provides a consistent way to get a human-readable identifier for the handler type.
    /// The name corresponds to the handler type as specified in manifest files and is used in
    /// logging, debugging, and error messages.
    ///
    /// ## Returns
    ///
    /// `&str` - The name of the handler
    ///
    /// ## Implementation Notes
    ///
    /// The names returned by this method should match the handler type strings used in
    /// actor manifests to ensure consistent identification throughout the system.
    pub fn name(&self) -> &str {
        // Method implementation...
    }
}
```
