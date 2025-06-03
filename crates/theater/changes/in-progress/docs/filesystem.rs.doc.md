
# FileSystem Host Module Documentation

## FileSystemHost Struct

```rust
/// # FileSystemHost
///
/// A host implementation that provides controlled filesystem access to WebAssembly actors.
///
/// ## Purpose
///
/// FileSystemHost provides a sandboxed interface for WebAssembly actors to interact with
/// the host's filesystem in a controlled, isolated manner. It gives actors the ability to 
/// perform common file operations like reading, writing, and listing directory contents,
/// as well as executing approved commands, while restricting these operations to a
/// designated directory tree.
///
/// This implementation serves as the host-side counterpart to the WebAssembly interface
/// defined in `filesystem.wit`, translating between the WebAssembly interface and actual
/// filesystem operations on the host.
///
/// ## Example
///
/// ```rust
/// use theater::host::filesystem::FileSystemHost;
/// use theater::config::FileSystemHandlerConfig;
/// use theater::actor_handle::ActorHandle;
/// use theater::shutdown::ShutdownReceiver;
/// use theater::wasm::ActorComponent;
/// 
/// async fn example() -> anyhow::Result<()> {
///     // Create a filesystem host with a specific directory as its root
///     let config = FileSystemHandlerConfig {
///         path: Some("/tmp/actor_files".to_string()),
///         new_dir: Some(false),
///         allowed_commands: Some(vec!["ls".to_string(), "cat".to_string()]),
///     };
///     
///     let fs_host = FileSystemHost::new(config);
///     
///     // Set up host functions for an actor component
///     let mut actor_component = ActorComponent::dummy(); // For demonstration
///     fs_host.setup_host_functions(&mut actor_component).await?;
///     
///     // Start the filesystem host
///     let actor_handle = ActorHandle::dummy(); // For demonstration
///     let shutdown_receiver = ShutdownReceiver::dummy(); // For demonstration
///     fs_host.start(actor_handle, shutdown_receiver).await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Parameters
///
/// The FileSystemHost is configured via a `FileSystemHandlerConfig` which specifies:
/// * `path` - Base directory for all filesystem operations (root of the sandbox)
/// * `new_dir` - Whether to create a new temporary directory
/// * `allowed_commands` - Optional list of commands that can be executed
///
/// ## Security
///
/// FileSystemHost implements critical security boundaries for filesystem access:
///
/// 1. **Path Sandboxing**: All operations are restricted to the configured base directory.
///    Path traversal attacks (e.g., using `../`) are prevented by resolving all paths
///    relative to this base directory.
///
/// 2. **Command Execution Control**: If command execution is enabled, only explicitly
///    allowed commands can be run, and arguments are carefully validated.
///
/// 3. **Event Tracking**: All filesystem operations are recorded in the event chain,
///    providing a comprehensive audit trail of actor filesystem activity.
///
/// 4. **Error Isolation**: Filesystem errors are handled gracefully and don't expose
///    sensitive system information to actors.
///
/// ## Implementation Notes
///
/// The FileSystemHost translates between the WebAssembly interface and actual filesystem
/// operations using the `func_wrap` and `func_wrap_async` methods provided by Wasmtime.
/// Each filesystem operation is wrapped in a function that performs validation, executes
/// the operation, logs the event, and translates the result back to the WebAssembly interface.
///
/// The implementation uses `PathBuf` to safely handle path operations and prevent path
/// traversal vulnerabilities.
pub struct FileSystemHost {
    path: PathBuf,
    allowed_commands: Option<Vec<String>>,
}
```

## Core Methods

```rust
impl FileSystemHost {
    /// # New
    ///
    /// Creates a new FileSystemHost with the specified configuration.
    ///
    /// ## Purpose
    ///
    /// This constructor initializes a new FileSystemHost instance with the given configuration,
    /// setting up the root directory for filesystem operations and any command execution permissions.
    ///
    /// ## Parameters
    ///
    /// * `config` - A `FileSystemHandlerConfig` specifying the base directory and other options
    ///
    /// ## Returns
    ///
    /// A new `FileSystemHost` instance
    ///
    /// ## Security
    ///
    /// The path specified in the configuration becomes the sandbox root for all filesystem operations.
    /// If `new_dir` is set to true, a secure temporary directory is created instead of using the
    /// provided path.
    ///
    /// ## Implementation Notes
    ///
    /// If `new_dir` is true, a random temporary directory is created under `/tmp/theater/` to
    /// ensure isolation between different actor instances.
    pub fn new(config: FileSystemHandlerConfig) -> Self {
        // Method implementation...
    }

    /// # Create Temporary Directory
    ///
    /// Creates a new, uniquely named temporary directory for filesystem operations.
    ///
    /// ## Purpose
    ///
    /// This method generates a secure, isolated temporary directory to serve as the
    /// root for an actor's filesystem operations. Using a temporary directory provides
    /// better isolation between actors and simplifies cleanup.
    ///
    /// ## Returns
    ///
    /// `Result<PathBuf>` - The path to the created temporary directory
    ///
    /// ## Security
    ///
    /// The temporary directory is created with a random name to prevent predictability.
    /// This method ensures each actor gets its own isolated filesystem space.
    ///
    /// ## Implementation Notes
    ///
    /// The temporary directory is created under `/tmp/theater/` with a random number
    /// as its name to ensure uniqueness.
    pub fn create_temp_dir() -> Result<PathBuf> {
        // Method implementation...
    }

    /// # Setup Host Functions
    ///
    /// Configures the WebAssembly component with filesystem host functions.
    ///
    /// ## Purpose
    ///
    /// This method registers all the filesystem-related host functions that actors can call
    /// through the WebAssembly component model interface. It creates a bridge between
    /// the WebAssembly interface defined in `filesystem.wit` and the actual filesystem
    /// operations on the host system.
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
    /// All functions registered by this method implement security checks including:
    /// - Path validation to ensure operations are contained within the allowed directory
    /// - Command whitelisting for execute-command operations
    /// - Event logging for audit purposes
    /// - Error handling that avoids information leakage
    ///
    /// ## Implementation Notes
    ///
    /// This method sets up several host functions in the theater:simple/filesystem namespace:
    /// - read-file: Read file contents from the filesystem
    /// - write-file: Write content to a file
    /// - list-files: List directory contents
    /// - delete-file: Remove a file
    /// - create-dir: Create a directory
    /// - delete-dir: Remove a directory and its contents
    /// - path-exists: Check if a path exists
    /// - execute-command: Execute a whitelisted command in the sandbox
    /// - execute-nix-command: Execute a command through nix-develop
    ///
    /// Each function is wrapped to handle input validation, error handling, and event recording.
    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        // Method implementation...
    }

    /// # Add Export Functions
    ///
    /// Adds actor export functions for filesystem operations.
    ///
    /// ## Purpose
    ///
    /// This method would register functions that the host can call on the actor's exports
    /// for filesystem-related callbacks. However, the FileSystemHost currently doesn't require
    /// any callbacks to the actor, so this method is a no-op.
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
    /// This is a placeholder for potential future expansion. Currently, filesystem operations
    /// are one-way (actor calls host) with no need for callbacks.
    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        // Method implementation...
    }

    /// # Start
    ///
    /// Starts the FileSystemHost handler.
    ///
    /// ## Purpose
    ///
    /// This method initializes the FileSystemHost and prepares it for handling filesystem
    /// operations from the actor. It logs the filesystem path that's being used and
    /// completes any necessary initialization.
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
    /// The FileSystemHost doesn't currently require background tasks, so this method
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

## Helper Functions

```rust
/// # Execute Command
///
/// Executes a command with the given arguments in the specified directory.
///
/// ## Purpose
///
/// This helper function provides a controlled way to execute system commands on behalf
/// of a WebAssembly actor. It enforces strict security policies to prevent abuse.
///
/// ## Parameters
///
/// * `allowed_path` - The root path that the command is allowed to operate in
/// * `dir` - The directory to execute the command in
/// * `cmd` - The command to execute
/// * `args` - The arguments to pass to the command
///
/// ## Returns
///
/// `Result<CommandResult>` - The result of the command execution
///
/// ## Security
///
/// This function implements several security measures:
/// - Validates that the directory is within the allowed path
/// - Strictly limits which commands can be executed (currently only 'nix')
/// - Validates command arguments against an allowed list
/// - Captures and sanitizes command output
///
/// ## Implementation Notes
///
/// Command execution is highly restricted, currently only allowing specific nix-related
/// commands with pre-approved argument patterns. This is intentionally conservative
/// to prevent command injection or system abuse.
async fn execute_command(
    allowed_path: PathBuf,
    dir: &Path,
    cmd: &str,
    args: &[&str],
) -> Result<CommandResult> {
    // Function implementation...
}

/// # Execute Nix Command
///
/// Executes a command through the nix develop environment.
///
/// ## Purpose
///
/// This helper function provides a way to execute commands in a nix development shell.
/// It's a convenience wrapper around execute_command that sets up the nix environment.
///
/// ## Parameters
///
/// * `allowed_path` - The root path that the command is allowed to operate in
/// * `dir` - The directory to execute the command in
/// * `command` - The command to execute in the nix shell
///
/// ## Returns
///
/// `Result<CommandResult>` - The result of the command execution
///
/// ## Security
///
/// This function inherits the security measures from `execute_command` and further
/// restricts execution to the nix develop environment, providing additional isolation.
///
/// ## Implementation Notes
///
/// This function wraps the provided command with `nix develop --command`, executing
/// it within a nix shell environment. This is particularly useful for building
/// WebAssembly components that require the nix environment.
async fn execute_nix_command(
    allowed_path: PathBuf,
    dir: &Path,
    command: &str,
) -> Result<CommandResult> {
    // Function implementation...
}
```

## Error Types

```rust
/// # FileSystemError
///
/// Error types specific to filesystem operations.
///
/// ## Purpose
///
/// This error enum defines the various types of errors that can occur during
/// filesystem operations, providing detailed context for troubleshooting.
///
/// ## Example
///
/// ```rust
/// use theater::host::filesystem::FileSystemError;
/// use std::io::Error as IoError;
/// 
/// fn example() {
///     // Create a path error
///     let path_error = FileSystemError::PathError("Invalid path".to_string());
///     
///     // Create an IO error
///     let io_error = FileSystemError::IoError(IoError::new(
///         std::io::ErrorKind::NotFound,
///         "File not found"
///     ));
///     
///     // Handle different error types
///     match path_error {
///         FileSystemError::PathError(msg) => println!("Path error: {}", msg),
///         FileSystemError::IoError(e) => println!("IO error: {}", e),
///         _ => println!("Other error"),
///     }
/// }
/// ```
///
/// ## Implementation Notes
///
/// This enum derives from `thiserror::Error` to provide consistent error handling
/// and formatting. It implements conversions from common error types to simplify
/// error propagation.
#[derive(Error, Debug)]
pub enum FileSystemError {
    #[error("Path error: {0}")]
    PathError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}
```
