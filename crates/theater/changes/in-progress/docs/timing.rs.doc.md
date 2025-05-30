
# Timing Host Module Documentation

## TimingHost Struct

```rust
/// # TimingHost
///
/// A host implementation that provides time-related functionality to WebAssembly actors.
///
/// ## Purpose
///
/// TimingHost provides WebAssembly actors with controlled access to timing operations,
/// such as retrieving the current time and scheduling delayed operations. It serves as
/// a bridge between the WebAssembly interface defined in `timing.wit` and the actual
/// time-related capabilities of the host system.
///
/// This implementation ensures that timing operations are safe, traceable, and properly
/// constrained to prevent abuse such as denial-of-service attacks through excessive sleeps.
///
/// ## Example
///
/// ```rust
/// use theater::host::timing::TimingHost;
/// use theater::config::TimingHostConfig;
/// use theater::actor_handle::ActorHandle;
/// use theater::shutdown::ShutdownReceiver;
/// use theater::wasm::ActorComponent;
/// 
/// async fn example() -> anyhow::Result<()> {
///     // Create a timing host with specific constraints
///     let config = TimingHostConfig {
///         max_sleep_duration: 60000, // 60 seconds
///         min_sleep_duration: 5,     // 5 milliseconds
///     };
///     
///     let timing_host = TimingHost::new(config);
///     
///     // Set up host functions for an actor component
///     let mut actor_component = ActorComponent::dummy(); // For demonstration
///     timing_host.setup_host_functions(&mut actor_component).await?;
///     
///     // Start the timing host
///     let actor_handle = ActorHandle::dummy(); // For demonstration
///     let shutdown_receiver = ShutdownReceiver::dummy(); // For demonstration
///     timing_host.start(actor_handle, shutdown_receiver).await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Parameters
///
/// The TimingHost is configured via a `TimingHostConfig` which specifies:
/// * `max_sleep_duration` - Maximum allowed sleep duration in milliseconds
/// * `min_sleep_duration` - Minimum allowed sleep duration in milliseconds
///
/// ## Security
///
/// TimingHost implements security controls for time-related operations:
///
/// 1. **Bounded Sleep Duration**: Sleep operations are constrained by minimum and maximum
///    duration limits to prevent abuse such as very short sleeps causing excessive CPU usage
///    or very long sleeps causing resource exhaustion.
///
/// 2. **Event Tracking**: All timing operations are recorded in the event chain,
///    providing a comprehensive audit trail of actor timing activities.
///
/// 3. **Safe Time Operations**: Time retrieval is performed in a safe manner that
///    doesn't expose system-specific time details.
///
/// ## Implementation Notes
///
/// The TimingHost translates between the WebAssembly interface and actual timing
/// operations using the `func_wrap` and `func_wrap_async` methods provided by Wasmtime.
/// Each timing operation is wrapped in a function that performs validation, executes
/// the operation, logs the event, and translates the result back to the WebAssembly interface.
pub struct TimingHost {
    config: TimingHostConfig,
}
```

## TimingError Enum

```rust
/// # TimingError
///
/// Error types specific to timing operations.
///
/// ## Purpose
///
/// This error enum defines the various types of errors that can occur during
/// timing operations, providing detailed context for troubleshooting.
///
/// ## Example
///
/// ```rust
/// use theater::host::timing::TimingError;
/// 
/// fn example() {
///     // Create a duration too long error
///     let too_long = TimingError::DurationTooLong {
///         duration: 120000,
///         max: 60000,
///     };
///     
///     // Create a duration too short error
///     let too_short = TimingError::DurationTooShort {
///         duration: 1,
///         min: 5,
///     };
///     
///     // Handle different error types
///     match too_long {
///         TimingError::DurationTooLong { duration, max } => {
///             println!("Duration {} ms exceeds maximum of {} ms", duration, max)
///         },
///         TimingError::DurationTooShort { duration, min } => {
///             println!("Duration {} ms is below minimum of {} ms", duration, min)
///         },
///         _ => println!("Other error"),
///     }
/// }
/// ```
///
/// ## Implementation Notes
///
/// This enum derives from `thiserror::Error` to provide consistent error handling
/// and formatting. It includes detailed error variants for specific timing issues
/// like durations being outside the allowed range.
#[derive(Error, Debug)]
pub enum TimingError {
    #[error("Timing error: {0}")]
    TimingError(String),

    #[error("Duration too long: {duration} ms exceeds maximum of {max} ms")]
    DurationTooLong { duration: u64, max: u64 },

    #[error("Duration too short: {duration} ms is below minimum of {min} ms")]
    DurationTooShort { duration: u64, min: u64 },

    #[error("Invalid deadline: {timestamp} is in the past")]
    InvalidDeadline { timestamp: u64 },

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),
}
```

## Core Methods

```rust
impl TimingHost {
    /// # New
    ///
    /// Creates a new TimingHost with the specified configuration.
    ///
    /// ## Purpose
    ///
    /// This constructor initializes a new TimingHost instance with the given configuration,
    /// setting up the constraints for timing operations like sleep durations.
    ///
    /// ## Parameters
    ///
    /// * `config` - A `TimingHostConfig` specifying timing constraints
    ///
    /// ## Returns
    ///
    /// A new `TimingHost` instance
    ///
    /// ## Implementation Notes
    ///
    /// The configuration includes bounds for sleep durations which are enforced
    /// by the host functions that this TimingHost sets up.
    pub fn new(config: TimingHostConfig) -> Self {
        // Method implementation...
    }

    /// # Setup Host Functions
    ///
    /// Configures the WebAssembly component with timing-related host functions.
    ///
    /// ## Purpose
    ///
    /// This method registers all the timing-related host functions that actors can call
    /// through the WebAssembly component model interface. It creates a bridge between
    /// the WebAssembly interface defined in `timing.wit` and the actual timing
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
    /// All timing functions implement security checks including:
    /// - Duration validation to prevent excessively long or short sleeps
    /// - Event logging for audit purposes
    /// - Error handling that avoids information leakage
    ///
    /// ## Implementation Notes
    ///
    /// This method sets up several host functions in the ntwk:theater/timing namespace:
    /// - now: Get the current timestamp in milliseconds
    /// - sleep: Pause execution for a specified duration
    /// - deadline: Pause execution until a specific timestamp
    ///
    /// Each function is wrapped to handle validation, error handling, and event recording.
    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        // Method implementation...
    }

    /// # Add Export Functions
    ///
    /// Adds actor export functions for timing operations.
    ///
    /// ## Purpose
    ///
    /// This method would register functions that the host can call on the actor's exports
    /// for timing-related callbacks. However, the TimingHost currently doesn't require
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
    /// This is a placeholder for potential future expansion. Currently, timing operations
    /// are one-way (actor calls host) with no need for callbacks.
    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        // Method implementation...
    }

    /// # Start
    ///
    /// Starts the TimingHost handler.
    ///
    /// ## Purpose
    ///
    /// This method initializes the TimingHost and prepares it for handling timing
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
    /// The TimingHost doesn't currently require background tasks, so this method
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

### Now Function

```rust
/// # Now
///
/// Returns the current timestamp in milliseconds since the Unix epoch.
///
/// ## Purpose
///
/// This host function allows WebAssembly actors to get the current system time.
/// It provides a consistent time reference for actors to coordinate their activities
/// or implement time-dependent logic.
///
/// ## Parameters
///
/// None
///
/// ## Returns
///
/// `u64` - Current timestamp in milliseconds since the Unix epoch
///
/// ## Security
///
/// This function is safe to expose to WebAssembly actors as it only provides
/// time information and cannot be used for resource exhaustion attacks.
///
/// ## Implementation Notes
///
/// The implementation uses `chrono::Utc::now().timestamp_millis()` to get the
/// current time in a consistent, platform-independent way. Each call is recorded
/// in the event chain for audit purposes.
pub fn now(ctx: StoreContextMut<'_, ActorStore>, ()): Result<(u64,)> {
    // Function implementation...
}
```

### Sleep Function

```rust
/// # Sleep
///
/// Pauses execution for the specified duration in milliseconds.
///
/// ## Purpose
///
/// This host function allows WebAssembly actors to introduce controlled delays
/// in their execution, which is useful for implementing rate-limiting, retry
/// mechanisms, or periodic tasks.
///
/// ## Parameters
///
/// * `duration` - The duration to sleep in milliseconds
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message if the sleep failed
///
/// ## Security
///
/// This function enforces bounds on the sleep duration to prevent abuse:
/// - Maximum duration: Configured via `max_sleep_duration` to prevent long-running sleeps
/// - Minimum duration: Configured via `min_sleep_duration` to prevent excessive CPU usage
///
/// ## Implementation Notes
///
/// The implementation uses `tokio::time::sleep` to perform the asynchronous sleep
/// operation, which doesn't block the entire runtime. Each sleep request is validated
/// against the configured bounds and recorded in the event chain for audit purposes.
pub fn sleep(ctx: StoreContextMut<'_, ActorStore>, (duration,): (u64,)): Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```

### Deadline Function

```rust
/// # Deadline
///
/// Pauses execution until the specified timestamp is reached.
///
/// ## Purpose
///
/// This host function allows WebAssembly actors to schedule activities for a
/// specific time, which is useful for implementing time-triggered events or
/// coordinating actions across multiple actors.
///
/// ## Parameters
///
/// * `timestamp` - The target timestamp in milliseconds since the Unix epoch
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message if the wait failed
///
/// ## Security
///
/// This function validates that the target timestamp is not too far in the future,
/// enforcing the same maximum duration constraint as the sleep function to prevent
/// resource exhaustion attacks.
///
/// ## Implementation Notes
///
/// The implementation calculates the duration between now and the target timestamp,
/// then uses `tokio::time::sleep` to perform the asynchronous wait. If the timestamp
/// is in the past, it returns immediately without error. Each deadline request is
/// validated and recorded in the event chain for audit purposes.
pub fn deadline(ctx: StoreContextMut<'_, ActorStore>, (timestamp,): (u64,)): Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```
