
# Message Server Host Module Documentation

## MessageServerHost Struct

```rust
/// # MessageServerHost
///
/// A host implementation that provides message passing and communication capabilities to WebAssembly actors.
///
/// ## Purpose
///
/// The MessageServerHost is a core component of the Theater system's actor communication model.
/// It enables actors to exchange messages with each other through various communication patterns:
///
/// 1. **Fire-and-forget Messages**: One-way messages sent to other actors without expecting a response
/// 2. **Request-Response**: Messages that expect a response from the recipient actor
/// 3. **Channels**: Persistent bi-directional communication streams between actors
///
/// This implementation serves as the bridge between the WebAssembly interface defined in
/// `message-server.wit` and the actual message passing mechanisms in the Theater runtime.
///
/// ## Example
///
/// ```rust
/// use theater::host::message_server::MessageServerHost;
/// use theater::actor_handle::ActorHandle;
/// use theater::messages::{ActorMessage, TheaterCommand};
/// use theater::shutdown::ShutdownReceiver;
/// use theater::wasm::ActorComponent;
/// use tokio::sync::mpsc;
/// 
/// async fn example() -> anyhow::Result<()> {
///     // Create message channels
///     let (mailbox_tx, mailbox_rx) = mpsc::channel(32);
///     let (theater_tx, _) = mpsc::channel(32);
///     
///     // Create a message server host
///     let mut message_server = MessageServerHost::new(mailbox_tx, mailbox_rx, theater_tx);
///     
///     // Set up host functions for an actor component
///     let mut actor_component = ActorComponent::dummy(); // For demonstration
///     message_server.setup_host_functions(&mut actor_component).await?;
///     
///     // Add export functions to an actor instance
///     let mut actor_instance = ActorInstance::dummy(); // For demonstration
///     message_server.add_export_functions(&mut actor_instance).await?;
///     
///     // Start the message server host
///     let actor_handle = ActorHandle::dummy(); // For demonstration
///     let shutdown_receiver = ShutdownReceiver::dummy(); // For demonstration
///     message_server.start(actor_handle, shutdown_receiver).await?;
///     
///     Ok(())
/// }
/// ```
///
/// ## Parameters
///
/// The MessageServerHost requires several channels for operation:
/// * `mailbox_tx`: Sender for the actor's mailbox
/// * `mailbox_rx`: Receiver for the actor's mailbox
/// * `theater_tx`: Sender for Theater runtime commands
///
/// ## Security
///
/// The MessageServerHost implements security controls for actor messaging:
///
/// 1. **Isolation**: Actors can only communicate through explicit message passing,
///    preventing shared memory or direct access to other actors' state.
///
/// 2. **Event Tracking**: All message operations are recorded in the event chain,
///    providing a comprehensive audit trail of inter-actor communication.
///
/// 3. **Channel Management**: Channels are explicitly opened, managed, and closed,
///    with both parties having control over their participation.
///
/// ## Implementation Notes
///
/// The MessageServerHost links the WebAssembly actor code with the Theater runtime's
/// message passing infrastructure. It handles both the host-side functions that actors
/// can call to send messages and the callbacks from the runtime when messages are received.
///
/// The implementation uses tokio channels for asynchronous message passing and maintains
/// state about active communication channels. The event loop in `start()` processes
/// incoming messages and routes them to the appropriate actor callbacks.
pub struct MessageServerHost {
    mailbox_tx: Sender<ActorMessage>,
    mailbox_rx: Receiver<ActorMessage>,
    theater_tx: Sender<TheaterCommand>,
    active_channels: HashMap<ChannelId, ChannelState>,
}
```

## Helper Types

```rust
/// # ChannelState
///
/// A structure representing the current state of a communication channel.
///
/// ## Purpose
///
/// This structure tracks the state of active channels, particularly whether
/// they are currently open. This information is used to determine if messages
/// can still be sent on a channel or if it has been closed.
///
/// ## Implementation Notes
///
/// Currently, this structure only tracks a boolean `is_open` flag, but it
/// could be extended in the future to include more channel state information
/// such as message counts, timestamps, or other metadata.
struct ChannelState {
    is_open: bool,
}

/// # ChannelAccept
///
/// A structure for actor responses to channel open requests.
///
/// ## Purpose
///
/// When an actor receives a channel open request, it responds with this
/// structure to indicate whether it accepts the channel and optionally
/// to include an initial response message.
///
/// ## Example
///
/// ```rust
/// use theater::host::message_server::ChannelAccept;
/// 
/// // Accept a channel with a response message
/// let accept = ChannelAccept {
///     accepted: true,
///     message: Some(b"Hello, I accept your channel request".to_vec()),
/// };
/// 
/// // Reject a channel
/// let reject = ChannelAccept {
///     accepted: false,
///     message: Some(b"Sorry, I cannot accept your channel request".to_vec()),
/// };
/// ```
///
/// ## Implementation Notes
///
/// This type derives from ComponentType, Lift, and Lower to enable it to be
/// used as a parameter or return value in WebAssembly component interfaces.
#[derive(Debug, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
struct ChannelAccept {
    accepted: bool,
    message: Option<Vec<u8>>,
}
```

## MessageServerError Enum

```rust
/// # MessageServerError
///
/// Error types specific to message server operations.
///
/// ## Purpose
///
/// This error enum defines the various types of errors that can occur during
/// message passing operations, providing detailed context for troubleshooting.
///
/// ## Example
///
/// ```rust
/// use theater::host::message_server::MessageServerError;
/// 
/// fn example() {
///     // Create a handler error
///     let handler_error = MessageServerError::HandlerError(
///         "Failed to deliver message".to_string()
///     );
///     
///     // Handle the error
///     match handler_error {
///         MessageServerError::HandlerError(msg) => println!("Handler error: {}", msg),
///         MessageServerError::ActorError(e) => println!("Actor error: {}", e),
///         _ => println!("Other error"),
///     }
/// }
/// ```
///
/// ## Implementation Notes
///
/// This enum derives from `thiserror::Error` to provide consistent error handling
/// and formatting. It includes error variants for messaging-specific issues and
/// implements conversions from common error types to simplify error propagation.
#[derive(Error, Debug)]
pub enum MessageServerError {
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
impl MessageServerHost {
    /// # New
    ///
    /// Creates a new MessageServerHost with the specified channels.
    ///
    /// ## Purpose
    ///
    /// This constructor initializes a new MessageServerHost instance with the given
    /// communication channels. The host uses these channels to send and receive
    /// messages between actors and the Theater runtime.
    ///
    /// ## Parameters
    ///
    /// * `mailbox_tx` - A sender for the actor's mailbox
    /// * `mailbox_rx` - A receiver for the actor's mailbox
    /// * `theater_tx` - A sender for Theater runtime commands
    ///
    /// ## Returns
    ///
    /// A new `MessageServerHost` instance
    ///
    /// ## Implementation Notes
    ///
    /// The constructor initializes the active_channels HashMap to track open channels.
    /// The mailbox channels are used for incoming messages to the actor, while the
    /// theater_tx channel is used to send commands to the Theater runtime.
    pub fn new(mailbox_tx: Sender<ActorMessage>, mailbox_rx: Receiver<ActorMessage>, theater_tx: Sender<TheaterCommand>) -> Self {
        // Method implementation...
    }

    /// # Setup Host Functions
    ///
    /// Configures the WebAssembly component with message-related host functions.
    ///
    /// ## Purpose
    ///
    /// This method registers all the message-related host functions that actors can call
    /// through the WebAssembly component model interface. It creates a bridge between
    /// the WebAssembly interface defined in `message-server.wit` and the actual message
    /// passing operations in the Theater runtime.
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
    /// All message functions implement security checks including:
    /// - Actor ID validation to ensure messages are sent to valid targets
    /// - Event logging for audit purposes
    /// - Error handling that avoids information leakage
    ///
    /// ## Implementation Notes
    ///
    /// This method sets up several host functions in the ntwk:theater/message-server-host namespace:
    /// - send: Send a one-way message to another actor
    /// - request: Send a request to another actor and wait for a response
    /// - open-channel: Establish a bi-directional communication channel with another actor
    /// - send-on-channel: Send a message on an established channel
    /// - close-channel: Close a communication channel
    ///
    /// Each function is wrapped to handle validation, error handling, and event recording.
    pub async fn setup_host_functions(&mut self, actor_component: &mut ActorComponent) -> Result<()> {
        // Method implementation...
    }

    /// # Add Export Functions
    ///
    /// Registers actor export functions for message handling.
    ///
    /// ## Purpose
    ///
    /// This method registers the functions that the runtime will call on the actor
    /// when it receives messages. These export functions form the interface that
    /// the actor must implement to handle incoming messages, requests, and channel
    /// operations.
    ///
    /// ## Parameters
    ///
    /// * `actor_instance` - A mutable reference to the ActorInstance
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Success or an error if export functions could not be registered
    ///
    /// ## Implementation Notes
    ///
    /// This method registers several export functions in the ntwk:theater/message-server-client namespace:
    /// - handle-send: Called when the actor receives a one-way message
    /// - handle-request: Called when the actor receives a request that requires a response
    /// - handle-channel-open: Called when another actor requests to open a channel
    /// - handle-channel-message: Called when a message is received on a channel
    /// - handle-channel-close: Called when a channel is closed
    ///
    /// The actor must implement these functions to handle the corresponding message types.
    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        // Method implementation...
    }

    /// # Start
    ///
    /// Starts the MessageServerHost message processing loop.
    ///
    /// ## Purpose
    ///
    /// This method initiates the main event loop for the MessageServerHost, which
    /// monitors the mailbox for incoming messages and processes them. It continues
    /// running until a shutdown signal is received or the mailbox channel is closed.
    ///
    /// ## Parameters
    ///
    /// * `actor_handle` - A handle to the actor that this handler is associated with
    /// * `shutdown_receiver` - A receiver for shutdown signals to cleanly terminate
    ///
    /// ## Returns
    ///
    /// `Result<()>` - Success or an error if the handler could not be started
    ///
    /// ## Implementation Notes
    ///
    /// The implementation uses tokio's select! macro to concurrently monitor the
    /// shutdown signal and incoming messages. When a message is received, it's
    /// processed by calling the process_message method. The loop continues until
    /// a shutdown signal is received or the mailbox channel is closed.
    pub async fn start(&mut self, actor_handle: ActorHandle, mut shutdown_receiver: ShutdownReceiver) -> Result<()> {
        // Method implementation...
    }

    /// # Process Message
    ///
    /// Processes an incoming message and dispatches it to the appropriate actor handler.
    ///
    /// ## Purpose
    ///
    /// This method handles incoming messages by invoking the appropriate export
    /// function on the actor based on the message type. It handles the different
    /// message patterns (one-way, request-response, channel) and manages channel state.
    ///
    /// ## Parameters
    ///
    /// * `msg` - The incoming message to process
    /// * `actor_handle` - A handle to the actor that should process the message
    ///
    /// ## Returns
    ///
    /// `Result<(), MessageServerError>` - Success or an error if message processing failed
    ///
    /// ## Implementation Notes
    ///
    /// The implementation uses pattern matching to handle different message types:
    /// - ActorMessage::Send: Calls the actor's handle-send function
    /// - ActorMessage::Request: Calls handle-request and returns the response
    /// - ActorMessage::ChannelOpen: Calls handle-channel-open and manages channel state
    /// - ActorMessage::ChannelMessage: Calls handle-channel-message for an open channel
    /// - ActorMessage::ChannelClose: Calls handle-channel-close and updates channel state
    /// - ActorMessage::ChannelInitiated: Updates channel state for newly opened channels
    ///
    /// The method ensures that messages are only delivered for valid, open channels
    /// and logs warnings for attempts to use unknown or closed channels.
    async fn process_message(
        &mut self,
        msg: ActorMessage,
        actor_handle: ActorHandle,
    ) -> Result<(), MessageServerError> {
        // Method implementation...
    }
}
```

## Registered Host Functions

### Send Function

```rust
/// # Send
///
/// Sends a one-way message to another actor.
///
/// ## Purpose
///
/// This host function allows actors to send fire-and-forget messages to other actors.
/// The message is delivered asynchronously, and no response is expected. This is
/// useful for simple notifications or commands that don't require confirmation.
///
/// ## Parameters
///
/// * `address` - The ID of the recipient actor
/// * `msg` - The message data to send
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message
///
/// ## Security
///
/// This function validates the recipient actor ID and records the message send
/// operation in the event chain for audit purposes. The operation is performed
/// asynchronously, and errors are properly handled and logged.
///
/// ## Implementation Notes
///
/// The implementation sends a TheaterCommand::SendMessage with an ActorMessage::Send
/// payload to the Theater runtime. The message is routed to the recipient actor's
/// mailbox, and the recipient's handle-send function is called to process it.
pub fn send(
    ctx: StoreContextMut<'_, ActorStore>,
    (address, msg): (String, Vec<u8>)
) -> Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```

### Request Function

```rust
/// # Request
///
/// Sends a request to another actor and waits for a response.
///
/// ## Purpose
///
/// This host function enables request-response communication between actors.
/// The sending actor blocks until it receives a response from the recipient
/// or an error occurs. This is useful for operations that require confirmation
/// or data retrieval from another actor.
///
/// ## Parameters
///
/// * `address` - The ID of the recipient actor
/// * `msg` - The request data to send
///
/// ## Returns
///
/// `Result<Vec<u8>, String>` - The response data or an error message
///
/// ## Security
///
/// This function validates the recipient actor ID and records both the request
/// and response operations in the event chain for audit purposes. The operation
/// uses a oneshot channel to receive the response, ensuring proper synchronization.
///
/// ## Implementation Notes
///
/// The implementation sends a TheaterCommand::SendMessage with an ActorMessage::Request
/// payload to the Theater runtime. The message includes a oneshot channel for the response.
/// The recipient actor's handle-request function is called to process the request and
/// generate a response, which is sent back through the channel.
pub fn request(
    ctx: StoreContextMut<'_, ActorStore>,
    (address, msg): (String, Vec<u8>)
) -> Future<Result<(Result<Vec<u8>, String>,)>> {
    // Function implementation...
}
```

### Open Channel Function

```rust
/// # Open Channel
///
/// Establishes a bi-directional communication channel with another actor.
///
/// ## Purpose
///
/// This host function allows actors to create persistent communication channels
/// with other actors. Unlike one-time messages or requests, channels provide a
/// long-lived connection for ongoing communication between actors. The recipient
/// actor must explicitly accept the channel request.
///
/// ## Parameters
///
/// * `address` - The ID of the target actor
/// * `initial_msg` - Initial message data to send with the channel request
///
/// ## Returns
///
/// `Result<String, String>` - The channel ID if accepted, or an error message
///
/// ## Security
///
/// This function validates the target actor ID and records the channel open
/// operation in the event chain for audit purposes. The channel ID is generated
/// based on the two participant IDs, ensuring uniqueness and proper identification.
///
/// ## Implementation Notes
///
/// The implementation sends a TheaterCommand::ChannelOpen command to the Theater runtime.
/// The target actor's handle-channel-open function is called to determine if it accepts
/// the channel request. If accepted, the channel ID is returned and both actors can
/// begin sending messages on the channel. If rejected, an error is returned to the caller.
pub fn open_channel(
    ctx: StoreContextMut<'_, ActorStore>,
    (address, initial_msg): (String, Vec<u8>)
) -> Future<Result<(Result<String, String>,)>> {
    // Function implementation...
}
```

### Send On Channel Function

```rust
/// # Send On Channel
///
/// Sends a message on an established channel.
///
/// ## Purpose
///
/// This host function allows actors to send messages over an existing communication
/// channel. Once a channel is established between two actors, both can use this
/// function to exchange messages on the channel.
///
/// ## Parameters
///
/// * `channel_id` - The ID of the channel to send on
/// * `msg` - The message data to send
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message
///
/// ## Security
///
/// This function validates the channel ID and records the channel message operation
/// in the event chain for audit purposes. The message is only delivered if the
/// channel exists and is open.
///
/// ## Implementation Notes
///
/// The implementation sends a TheaterCommand::ChannelMessage command to the Theater
/// runtime. The recipient actor's handle-channel-message function is called to process
/// the incoming channel message. The function includes the sender's ID to enable
/// the recipient to identify the message source.
pub fn send_on_channel(
    ctx: StoreContextMut<'_, ActorStore>,
    (channel_id, msg): (String, Vec<u8>)
) -> Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```

### Close Channel Function

```rust
/// # Close Channel
///
/// Closes an established communication channel.
///
/// ## Purpose
///
/// This host function allows actors to terminate a communication channel when
/// it's no longer needed. Once closed, no more messages can be sent on the channel,
/// and both parties are notified of the closure.
///
/// ## Parameters
///
/// * `channel_id` - The ID of the channel to close
///
/// ## Returns
///
/// `Result<(), String>` - Success or an error message
///
/// ## Security
///
/// This function validates the channel ID and records the channel close operation
/// in the event chain for audit purposes. Both participants are notified when a
/// channel is closed, ensuring proper cleanup.
///
/// ## Implementation Notes
///
/// The implementation sends a TheaterCommand::ChannelClose command to the Theater
/// runtime. Both actors are notified of the channel closure, and their respective
/// channel state is updated to reflect that the channel is no longer open. The
/// channel's handle-channel-close function is called to allow for any cleanup actions.
pub fn close_channel(
    ctx: StoreContextMut<'_, ActorStore>,
    (channel_id,): (String,)
) -> Future<Result<(Result<(), String>,)>> {
    // Function implementation...
}
```

## Actor Export Functions

### Handle Send

```rust
/// # Handle Send
///
/// Actor export function for processing incoming one-way messages.
///
/// ## Purpose
///
/// This export function is called on the actor when it receives a one-way message
/// from another actor. The actor implements this function to process such messages.
///
/// ## Parameters
///
/// * `data` - The message data
///
/// ## Returns
///
/// None (no result)
///
/// ## Implementation Notes
///
/// Actors must implement this function to handle incoming messages. The implementation
/// typically deserializes the message data, performs any necessary actions, and updates
/// the actor's state accordingly. Since this is a one-way message, no response is expected.
pub fn handle_send(data: Vec<u8>) {
    // Function implementation by the actor
}
```

### Handle Request

```rust
/// # Handle Request
///
/// Actor export function for processing incoming requests that require a response.
///
/// ## Purpose
///
/// This export function is called on the actor when it receives a request from
/// another actor. The actor implements this function to process the request and
/// generate a response.
///
/// ## Parameters
///
/// * `data` - The request data
///
/// ## Returns
///
/// `Vec<u8>` - The response data
///
/// ## Implementation Notes
///
/// Actors must implement this function to handle incoming requests. The implementation
/// typically deserializes the request data, performs the requested operation, and
/// serializes the response data. The response is sent back to the requesting actor.
pub fn handle_request(data: Vec<u8>) -> Vec<u8> {
    // Function implementation by the actor
}
```

### Handle Channel Open

```rust
/// # Handle Channel Open
///
/// Actor export function for handling incoming channel open requests.
///
/// ## Purpose
///
/// This export function is called on the actor when another actor requests to
/// open a communication channel. The actor implements this function to decide
/// whether to accept or reject the channel request.
///
/// ## Parameters
///
/// * `data` - The initial message data from the channel opener
///
/// ## Returns
///
/// `ChannelAccept` - A structure indicating whether the channel is accepted
///
/// ## Implementation Notes
///
/// Actors must implement this function to handle channel open requests. The implementation
/// typically deserializes the initial message, determines if the channel should be accepted
/// based on the message content or other factors, and returns a ChannelAccept structure.
/// If accepted, the actor can include an initial response message in the structure.
pub fn handle_channel_open(data: Vec<u8>) -> ChannelAccept {
    // Function implementation by the actor
}
```

### Handle Channel Message

```rust
/// # Handle Channel Message
///
/// Actor export function for processing messages received on a channel.
///
/// ## Purpose
///
/// This export function is called on the actor when it receives a message on an
/// established channel. The actor implements this function to process such messages.
///
/// ## Parameters
///
/// * `channel_id` - The ID of the channel the message was sent on
/// * `data` - The message data
///
/// ## Returns
///
/// None (no result)
///
/// ## Implementation Notes
///
/// Actors must implement this function to handle channel messages. The implementation
/// typically uses the channel ID to identify the communication context, deserializes
/// the message data, and performs any necessary actions. Responses are sent using the
/// send-on-channel host function rather than as a return value.
pub fn handle_channel_message(channel_id: String, data: Vec<u8>) {
    // Function implementation by the actor
}
```

### Handle Channel Close

```rust
/// # Handle Channel Close
///
/// Actor export function for handling channel closure notifications.
///
/// ## Purpose
///
/// This export function is called on the actor when a channel it participates in
/// is closed. The actor implements this function to perform any necessary cleanup.
///
/// ## Parameters
///
/// * `channel_id` - The ID of the closed channel
///
/// ## Returns
///
/// None (no result)
///
/// ## Implementation Notes
///
/// Actors must implement this function to handle channel closures. The implementation
/// typically uses the channel ID to identify which channel was closed, updates any
/// internal state related to the channel, and performs any necessary cleanup actions.
pub fn handle_channel_close(channel_id: String) {
    // Function implementation by the actor
}
```
