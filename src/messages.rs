use crate::actor::ActorError;
/// # Theater Message System
///
/// Defines the message types used for communication between different components
/// of the Theater system, including commands for the runtime, actor messages,
/// and channel-based communication.
///
/// ## Purpose
///
/// This module forms the core messaging infrastructure of Theater, defining the
/// protocol through which components communicate. It includes command messages for
/// the runtime to manage actors, messages for inter-actor communication, and the
/// channel system for streaming communication.
///
/// ## Example
///
/// ```rust
/// use theater::messages::{TheaterCommand, ActorStatus};
/// use theater::id::TheaterId;
/// use tokio::sync::oneshot;
///
/// async fn example() {
///     // Create a command to spawn a new actor
///     let (tx, rx) = oneshot::channel();
///     let spawn_cmd = TheaterCommand::SpawnActor {
///         manifest_path: "actor_manifest.toml".to_string(),
///         init_bytes: None,
///         response_tx: tx,
///         parent_id: None,
///     };
///
///     // Send the command to the runtime...
///     
///     // Wait for the response
///     let actor_id = rx.await.unwrap().unwrap();
///     
///     // Create a command to check actor status
///     let (status_tx, status_rx) = oneshot::channel();
///     let status_cmd = TheaterCommand::GetActorStatus {
///         actor_id,
///         response_tx: status_tx,
///     };
///     
///     // Send the command and wait for the response
///     let status = status_rx.await.unwrap().unwrap();
///     assert_eq!(status, ActorStatus::Running);
/// }
/// ```
///
/// ## Security
///
/// Messages in this module often cross security boundaries between actors
/// and between actors and the runtime. The message types are designed to
/// ensure that:
///
/// - Actors can only communicate with other actors through controlled channels
/// - Actor state and events are accessed only by authorized parties
/// - Commands requiring privileges are properly authenticated
/// - Channel communication preserves isolation between participants
///
/// ## Implementation Notes
///
/// The messaging system is built on top of Tokio's `mpsc` and `oneshot` channels
/// to provide asynchronous communication without blocking. Response channels
/// (`oneshot::Sender`) are used extensively to allow commands to return results
/// to their callers.
use crate::chain::ChainEvent;
use crate::config::ManifestConfig;
use crate::id::TheaterId;
use crate::metrics::ActorMetrics;
use crate::store::ContentStore;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

/// # Theater Command
///
/// Commands sent to the Theater runtime to manage actors and system resources.
///
/// ## Purpose
///
/// These commands form the control plane of the Theater system, allowing
/// clients to manage the lifecycle of actors, send messages between actors,
/// record events, and query system state.
///
/// ## Example
///
/// ```rust
/// use theater::messages::TheaterCommand;
/// use theater::id::TheaterId;
/// use tokio::sync::oneshot;
///
/// // Create a command to stop an actor
/// let (tx, rx) = oneshot::channel();
/// let actor_id = TheaterId::new();
/// let stop_command = TheaterCommand::StopActor {
///     actor_id,
///     response_tx: tx,
/// };
///
/// // Send the command to the runtime...
/// ```
///
/// ## Security
///
/// Commands that affect actor lifecycle can only be executed by the runtime
/// or by actors with appropriate supervision permissions. Response channels
/// ensure that command results are only returned to the original sender.
#[derive(Debug)]
pub enum TheaterCommand {
    /// # Spawn a new actor
    ///
    /// Creates a new actor from a manifest file and optional initialization data.
    ///
    /// ## Parameters
    ///
    /// * `manifest_path` - Path to the actor's manifest file
    /// * `init_bytes` - Optional initialization data to pass to the actor
    /// * `response_tx` - Channel to receive the result (actor ID or error)
    /// * `parent_id` - Optional parent actor ID for supervision hierarchy
    SpawnActor {
        manifest_path: String,
        init_bytes: Option<Vec<u8>>,
        response_tx: oneshot::Sender<Result<TheaterId>>,
        parent_id: Option<TheaterId>,
    },

    /// # Resume an existing actor
    ///
    /// Restarts an actor from a manifest and optionally restores its previous state.
    ///
    /// ## Parameters
    ///
    /// * `manifest_path` - Path to the actor's manifest file
    /// * `state_bytes` - Optional state data to restore
    /// * `response_tx` - Channel to receive the result (actor ID or error)
    /// * `parent_id` - Optional parent actor ID for supervision hierarchy
    ResumeActor {
        manifest_path: String,
        state_bytes: Option<Vec<u8>>,
        response_tx: oneshot::Sender<Result<TheaterId>>,
        parent_id: Option<TheaterId>,
    },

    /// # Stop an actor
    ///
    /// Gracefully stops a running actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to stop
    /// * `response_tx` - Channel to receive the result (success or error)
    StopActor {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<()>>,
    },

    /// # Update an actor's component
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to update
    /// * `component` - The new component address
    /// * `response_tx` - Channel to receive the result (success or error)
    UpdateActorComponent {
        actor_id: TheaterId,
        component: String,
        response_tx: oneshot::Sender<Result<()>>,
    },

    /// # Send a message to an actor
    ///
    /// Sends a message to a specific actor for processing.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to send the message to
    /// * `actor_message` - The message to send
    SendMessage {
        actor_id: TheaterId,
        actor_message: ActorMessage,
    },

    /// # Record a new event
    ///
    /// Records an event in an actor's event chain.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor the event relates to
    /// * `event` - The event to record
    NewEvent {
        actor_id: TheaterId,
        event: ChainEvent,
    },

    /// # Record an actor error
    ///
    /// Records an error event in an actor's event chain.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor that experienced the error
    /// * `event` - The error event to record
    ActorError {
        actor_id: TheaterId,
        error: ActorError,
    },

    /// # Get all actors
    ///
    /// Retrieves a list of all actor IDs in the system.
    ///
    /// ## Parameters
    ///
    /// * `response_tx` - Channel to receive the result (list of actor IDs)
    GetActors {
        response_tx: oneshot::Sender<Result<Vec<(TheaterId, String)>>>,
    },

    GetActorManifest {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<ManifestConfig>>,
    },

    /// # Get actor status
    ///
    /// Retrieves the current status of an actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to check
    /// * `response_tx` - Channel to receive the result (actor status)
    GetActorStatus {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<ActorStatus>>,
    },

    /// # List child actors
    ///
    /// Retrieves a list of all child actors for a given parent.
    ///
    /// ## Parameters
    ///
    /// * `parent_id` - ID of the parent actor
    /// * `response_tx` - Channel to receive the result (list of child actor IDs)
    ///
    /// ## Security
    ///
    /// This operation is only available to actors with supervision permissions
    /// or to the system itself.
    ListChildren {
        parent_id: TheaterId,
        response_tx: oneshot::Sender<Vec<TheaterId>>,
    },

    /// # Restart an actor
    ///
    /// Restarts a failed or stopped actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to restart
    /// * `response_tx` - Channel to receive the result (success or error)
    ///
    /// ## Security
    ///
    /// This operation is only available to the actor's supervisor or to the system itself.
    RestartActor {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<()>>,
    },

    /// # Get actor state
    ///
    /// Retrieves the current state of an actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to get state for
    /// * `response_tx` - Channel to receive the result (actor state data)
    ///
    /// ## Security
    ///
    /// This operation is only available to the actor's supervisor or to the system itself.
    GetActorState {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<Option<Vec<u8>>>>,
    },

    /// # Get actor events
    ///
    /// Retrieves the event history of an actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to get events for
    /// * `response_tx` - Channel to receive the result (list of events)
    ///
    /// ## Security
    ///
    /// This operation is only available to the actor's supervisor or to the system itself.
    GetActorEvents {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>>>,
    },

    /// # Get actor metrics
    ///
    /// Retrieves performance and resource usage metrics for an actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to get metrics for
    /// * `response_tx` - Channel to receive the result (actor metrics)
    GetActorMetrics {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<ActorMetrics>>,
    },

    /// # Subscribe to actor events
    ///
    /// Creates a subscription to receive all future events from an actor.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to subscribe to
    /// * `event_tx` - Channel to receive events as they occur
    ///
    /// ## Security
    ///
    /// This operation is only available to the actor's supervisor or to the system itself.
    SubscribeToActor {
        actor_id: TheaterId,
        event_tx: Sender<Result<ChainEvent, ActorError>>,
    },

    /// # Open a communication channel
    ///
    /// Opens a bidirectional communication channel between two participants.
    ///
    /// ## Parameters
    ///
    /// * `initiator_id` - The participant initiating the channel
    /// * `target_id` - The target participant for the channel
    /// * `channel_id` - The unique ID for this channel
    /// * `initial_message` - The first message to send on the channel
    /// * `response_tx` - Channel to receive the result (success or error)
    ChannelOpen {
        initiator_id: ChannelParticipant,
        target_id: ChannelParticipant,
        channel_id: ChannelId,
        initial_message: Vec<u8>,
        response_tx: oneshot::Sender<Result<bool>>,
    },

    /// # Send a message on a channel
    ///
    /// Sends data through an established channel.
    ///
    /// ## Parameters
    ///
    /// * `channel_id` - The ID of the channel to send on
    /// * `sender_id` - The participant sending the message
    /// * `message` - The message data to send
    ChannelMessage {
        channel_id: ChannelId,
        sender_id: ChannelParticipant,
        message: Vec<u8>,
    },

    /// # Close a channel
    ///
    /// Closes an open communication channel.
    ///
    /// ## Parameters
    ///
    /// * `channel_id` - The ID of the channel to close
    ChannelClose { channel_id: ChannelId },

    /// # List active channels
    ///
    /// Retrieves a list of all active communication channels.
    ///
    /// ## Parameters
    ///
    /// * `response_tx` - Channel to receive the result (list of channel IDs and participants)
    ///
    /// ## Security
    ///
    /// This operation is only available to the system or to actors with
    /// appropriate monitoring permissions.
    ListChannels {
        response_tx: oneshot::Sender<Result<Vec<(ChannelId, Vec<ChannelParticipant>)>>>,
    },

    /// # Get channel status
    ///
    /// Retrieves information about a specific channel.
    ///
    /// ## Parameters
    ///
    /// * `channel_id` - The ID of the channel to query
    /// * `response_tx` - Channel to receive the result (channel participant info)
    ///
    /// ## Security
    ///
    /// This operation is only available to participants in the channel,
    /// the system, or actors with appropriate monitoring permissions.
    GetChannelStatus {
        channel_id: ChannelId,
        response_tx: oneshot::Sender<Result<Option<Vec<ChannelParticipant>>>>,
    },

    /// # Register a new channel
    ///
    /// Registers a new channel in the system (internal use).
    ///
    /// ## Parameters
    ///
    /// * `channel_id` - The ID of the channel to register
    /// * `participants` - The participants in the channel
    ///
    /// ## Security
    ///
    /// This operation is only available to the system itself.
    RegisterChannel {
        channel_id: ChannelId,
        participants: Vec<ChannelParticipant>,
    },

    /// # Create a new content store
    ///
    /// Creates a new content-addressable storage instance.
    ///
    /// ## Parameters
    ///
    /// * `response_tx` - Channel to receive the result (new store instance)
    NewStore {
        response_tx: oneshot::Sender<Result<ContentStore>>,
    },
}

impl TheaterCommand {
    /// # Convert a command to a loggable string
    ///
    /// Converts a command to a human-readable string for logging purposes.
    ///
    /// ## Returns
    ///
    /// A string representation of the command suitable for logging
    pub fn to_log(&self) -> String {
        match self {
            TheaterCommand::SpawnActor { manifest_path, .. } => {
                format!("SpawnActor: {}", manifest_path)
            }
            TheaterCommand::ResumeActor { manifest_path, .. } => {
                format!("ResumeActor: {}", manifest_path)
            }
            TheaterCommand::UpdateActorComponent {
                actor_id,
                component,
                ..
            } => {
                format!("UpdateActorComponent: {} -> {}", actor_id, component)
            }
            TheaterCommand::StopActor { actor_id, .. } => {
                format!("StopActor: {:?}", actor_id)
            }
            TheaterCommand::SendMessage { actor_id, .. } => {
                format!("SendMessage: {:?}", actor_id)
            }
            TheaterCommand::NewEvent { actor_id, .. } => {
                format!("NewEvent: {:?}", actor_id)
            }
            TheaterCommand::ActorError { actor_id, .. } => {
                format!("ActorError: {:?}", actor_id)
            }
            TheaterCommand::GetActors { .. } => "GetActors".to_string(),
            TheaterCommand::GetActorManifest { actor_id, .. } => {
                format!("GetActorManifest: {:?}", actor_id)
            }
            TheaterCommand::GetActorStatus { actor_id, .. } => {
                format!("GetActorStatus: {:?}", actor_id)
            }
            TheaterCommand::ListChildren { parent_id, .. } => {
                format!("ListChildren: {:?}", parent_id)
            }
            TheaterCommand::RestartActor { actor_id, .. } => {
                format!("RestartActor: {:?}", actor_id)
            }
            TheaterCommand::GetActorState { actor_id, .. } => {
                format!("GetActorState: {:?}", actor_id)
            }
            TheaterCommand::GetActorEvents { actor_id, .. } => {
                format!("GetActorEvents: {:?}", actor_id)
            }
            TheaterCommand::GetActorMetrics { actor_id, .. } => {
                format!("GetActorMetrics: {:?}", actor_id)
            }
            TheaterCommand::SubscribeToActor { actor_id, .. } => {
                format!("SubscribeToActor: {:?}", actor_id)
            }
            TheaterCommand::ChannelOpen {
                initiator_id,
                target_id,
                channel_id,
                ..
            } => {
                format!(
                    "ChannelOpen: {} -> {} (channel: {})",
                    initiator_id, target_id, channel_id
                )
            }
            TheaterCommand::ChannelMessage { channel_id, .. } => {
                format!("ChannelMessage: {}", channel_id)
            }
            TheaterCommand::ChannelClose { channel_id } => {
                format!("ChannelClose: {}", channel_id)
            }
            TheaterCommand::ListChannels { .. } => "ListChannels".to_string(),
            TheaterCommand::GetChannelStatus { channel_id, .. } => {
                format!("GetChannelStatus: {}", channel_id)
            }
            TheaterCommand::RegisterChannel {
                channel_id,
                participants,
            } => {
                format!(
                    "RegisterChannel: {} with {} participants",
                    channel_id,
                    participants.len()
                )
            }
            TheaterCommand::NewStore { .. } => "NewStore".to_string(),
        }
    }
}

/// # Channel Identifier
///
/// A unique identifier for a communication channel between participants.
///
/// ## Purpose
///
/// ChannelId provides a stable, unique identifier for communication channels
/// between actors or between actors and external components. The identifier
/// is derived from the participants' identities and includes entropy to
/// ensure uniqueness.
///
/// ## Example
///
/// ```rust
/// use theater::messages::{ChannelId, ChannelParticipant};
/// use theater::id::TheaterId;
///
/// // Create participants
/// let actor_id = TheaterId::new();
/// let initiator = ChannelParticipant::Actor(actor_id);
/// let target = ChannelParticipant::External;
///
/// // Generate a channel ID
/// let channel_id = ChannelId::new(&initiator, &target);
/// println!("Created channel: {}", channel_id);
/// ```
///
/// ## Security
///
/// Channel IDs include cryptographic entropy to prevent guessing, ensuring
/// that only authorized participants can access a channel.
///
/// ## Implementation Notes
///
/// The Channel ID is constructed using a combination of:
/// - Hashes of both participant identities
/// - Current timestamp
/// - Random value
///
/// This provides strong uniqueness guarantees even with high channel creation rates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChannelId(pub String);

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ChannelId {
    /// # Create a new channel ID
    ///
    /// Generates a new unique channel ID based on the participants.
    ///
    /// ## Parameters
    ///
    /// * `initiator` - The participant initiating the channel
    /// * `target` - The target participant for the channel
    ///
    /// ## Returns
    ///
    /// A new unique ChannelId
    pub fn new(initiator: &ChannelParticipant, target: &ChannelParticipant) -> Self {
        let mut hasher = DefaultHasher::new();
        let timestamp = chrono::Utc::now().timestamp_millis();
        let rand_value: u64 = rand::random();

        initiator.hash(&mut hasher);
        target.hash(&mut hasher);
        timestamp.hash(&mut hasher);
        rand_value.hash(&mut hasher);

        let hash = hasher.finish();
        ChannelId(format!("ch_{:016x}", hash))
    }

    /// # Get the channel ID as a string
    ///
    /// Returns the string representation of the channel ID.
    ///
    /// ## Returns
    ///
    /// A string slice containing the channel ID
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// # Channel Participant
///
/// Represents an endpoint in a communication channel.
///
/// ## Purpose
///
/// ChannelParticipant identifies entities that can participate in channel-based
/// communication, either actors within the Theater system or external clients.
///
/// ## Example
///
/// ```rust
/// use theater::messages::ChannelParticipant;
/// use theater::id::TheaterId;
///
/// // Create an actor participant
/// let actor_id = TheaterId::new();
/// let participant = ChannelParticipant::Actor(actor_id);
///
/// // Create an external participant
/// let external = ChannelParticipant::External;
/// ```
///
/// ## Security
///
/// The participant type is used to enforce security boundaries:
/// - Actor participants can only be accessed by those actors
/// - External participants are authenticated through the Theater runtime
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelParticipant {
    /// An actor in the runtime
    Actor(TheaterId),
    /// An external client (like CLI)
    External,
}

impl std::fmt::Display for ChannelParticipant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelParticipant::Actor(actor_id) => write!(f, "Actor({})", actor_id),
            ChannelParticipant::External => write!(f, "External"),
        }
    }
}

/// # Actor Request
///
/// A request message sent to an actor that requires a response.
///
/// ## Purpose
///
/// ActorRequest represents a synchronous request-response interaction pattern,
/// where the sender expects a response from the actor. The actor processes the
/// data in the request and sends a response through the provided channel.
///
/// ## Implementation Notes
///
/// The data field contains serialized request parameters, typically in a format
/// that the actor knows how to parse (e.g., JSON, bincode, etc.).
#[derive(Debug)]
pub struct ActorRequest {
    /// Channel to send the response back to the requester
    pub response_tx: oneshot::Sender<Vec<u8>>,
    /// Request data (serialized parameters)
    pub data: Vec<u8>,
}

/// # Actor Send
///
/// A fire-and-forget message sent to an actor.
///
/// ## Purpose
///
/// ActorSend represents an asynchronous, one-way message pattern, where the sender
/// does not expect or wait for a response. The actor processes the message but does
/// not send anything back to the sender.
///
/// ## Implementation Notes
///
/// The data field contains serialized message parameters, typically in a format
/// that the actor knows how to parse (e.g., JSON, bincode, etc.).
#[derive(Debug)]
pub struct ActorSend {
    /// Message data (serialized parameters)
    pub data: Vec<u8>,
}

/// # Actor Channel Open Request
///
/// A request to open a new communication channel with an actor.
///
/// ## Purpose
///
/// ActorChannelOpen is used to establish a new bidirectional communication channel
/// with an actor. The actor can accept or reject the channel request.
///
/// ## Security
///
/// The actor validates the channel request and can reject unauthorized channel
/// establishment attempts.
#[derive(Debug)]
pub struct ActorChannelOpen {
    /// The unique ID for this channel
    pub channel_id: ChannelId,
    /// Channel to receive the result of the open request
    pub response_tx: oneshot::Sender<Result<bool>>,
    /// Initial message data (may contain authentication/metadata)
    pub data: Vec<u8>,
}

/// # Actor Channel Message
///
/// A message sent through an established channel to an actor.
///
/// ## Purpose
///
/// ActorChannelMessage represents a message sent through an already established
/// channel. These messages form the ongoing communication within a channel.
///
/// ## Security
///
/// Messages are only delivered if the channel is open and the sender is
/// an authorized participant.
#[derive(Debug)]
pub struct ActorChannelMessage {
    /// The ID of the channel to send on
    pub channel_id: ChannelId,
    /// Message data
    pub data: Vec<u8>,
}

/// # Actor Channel Close
///
/// A notification that a channel has been closed.
///
/// ## Purpose
///
/// ActorChannelClose represents a request to close a communication channel
/// or a notification that a channel has been closed by another participant.
///
/// ## Implementation Notes
///
/// Channel closure is final - once closed, a channel cannot be reopened.
/// A new channel must be established if communication is to resume.
#[derive(Debug)]
pub struct ActorChannelClose {
    /// The ID of the channel to close
    pub channel_id: ChannelId,
}

/// # Actor Channel Initiated
///
/// A notification that a new channel has been initiated with this actor.
///
/// ## Purpose
///
/// ActorChannelInitiated informs an actor that a new communication channel
/// has been opened where the actor is the target. The actor can begin
/// communicating on this channel immediately.
///
/// ## Implementation Notes
///
/// This message is generated by the runtime when another participant
/// successfully opens a channel with this actor.
#[derive(Debug)]
pub struct ActorChannelInitiated {
    /// The unique ID for this channel
    pub channel_id: ChannelId,
    /// The participant who opened the channel
    pub target_id: ChannelParticipant,
    /// The initial message sent on the channel
    pub initial_msg: Vec<u8>,
}

/// # Actor Message
///
/// Represents the different types of messages that can be sent to an actor.
///
/// ## Purpose
///
/// ActorMessage provides a unified type for all messages that can be sent to
/// actors, encompassing request-response interactions, one-way messages,
/// and channel-based communication.
///
/// ## Example
///
/// ```rust
/// use theater::messages::{ActorMessage, ActorSend};
///
/// // Create a simple message
/// let message_data = vec![1, 2, 3, 4]; // Some serialized data
/// let message = ActorMessage::Send(ActorSend {
///     data: message_data,
/// });
///
/// // This would then be sent to an actor...
/// ```
///
/// ## Security
///
/// The runtime validates that senders have permission to send messages
/// to the target actor before delivery.
#[derive(Debug)]
pub enum ActorMessage {
    /// Request-response interaction
    Request(ActorRequest),
    /// One-way message
    Send(ActorSend),
    /// Request to open a new channel
    ChannelOpen(ActorChannelOpen),
    /// Message on an established channel
    ChannelMessage(ActorChannelMessage),
    /// Request to close a channel
    ChannelClose(ActorChannelClose),
    /// Notification of a new channel
    ChannelInitiated(ActorChannelInitiated),
}

/// # Actor Status
///
/// Represents the current operational status of an actor.
///
/// ## Purpose
///
/// ActorStatus provides a standardized way to report the current state of an actor,
/// used by monitoring tools, supervisors, and the runtime to track actor health.
///
/// ## Example
///
/// ```rust
/// use theater::messages::ActorStatus;
///
/// // Check if an actor is functioning
/// fn is_actor_healthy(status: &ActorStatus) -> bool {
///     matches!(status, ActorStatus::Running)
/// }
/// ```
///
/// ## Implementation Notes
///
/// Status transitions are managed by the runtime and triggered by various events:
/// - System startup or explicit start commands transition to Running
/// - Clean shutdown requests transition to Stopped
/// - Errors or crashes transition to Failed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorStatus {
    /// Actor is active and processing messages
    Running,
    /// Actor has been stopped gracefully
    Stopped,
    /// Actor has experienced an error or crash
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildError {
    pub actor_id: TheaterId,
    pub error: ActorError,
}
