use crate::actor::handle::ActorHandle;
use crate::actor::ActorError;
use crate::actor::ActorRuntimeError;
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
///         name: Some("my-actor".to_string()),
///         manifest: None,
///         init_state: theater::pack_bridge::Value::Tuple(vec![]),
///         wasm_bytes: vec![], // WASM bytes would be loaded here
///         response_tx: tx,
///         subscription_tx: None,
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
use crate::id::TheaterId;
use crate::metrics::ActorMetrics;
use crate::pack_bridge::{InterfaceHash, Value, ValueType};

/// The conventional "no initial state" sentinel passed to a freshly-spawned
/// actor's `init` when no caller-provided state and no manifest
/// `initial_state` is available. Today this is `option<list<u8>>::none` —
/// the historical default the runtime has always given actors that don't
/// declare otherwise.
///
/// Callers building a `SpawnActor`/`SetupActor` command should call this
/// for the "no state" case rather than reconstructing the Value inline.
pub fn default_init_state() -> Value {
    Value::Option {
        inner_type: ValueType::List(Box::new(ValueType::U8)),
        value: None,
    }
}
use crate::store::ContentStore;
use crate::ManifestConfig;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

/// One row of the actor supervision tree: `(actor-id, name, parent-id)`.
/// `parent-id` is the spawning supervisor (`None` for root actors). Carried by
/// `GetActors` responses and by spawn-subscription notifications so consumers
/// can render the tree.
pub type ActorTreeRow = (TheaterId, String, Option<TheaterId>);

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
/// let actor_id = TheaterId::generate();
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
#[allow(clippy::large_enum_variant)]
pub enum TheaterCommand {
    /// # Spawn a new actor (setup + init)
    ///
    /// Creates a new actor from WASM bytes, sets up its task loops, then
    /// calls its `theater:simple/actor.init` export before responding. The
    /// caller gets the actor id only after init has completed successfully.
    ///
    /// ## Parameters
    ///
    /// * `wasm_bytes` - The WASM module bytes to instantiate
    /// * `name` - Optional actor name for debugging/logging
    /// * `manifest` - Optional manifest for handler configs, replay settings, etc.
    /// * `init_state` - Initial state passed to the actor's init. Takes
    ///   priority over `manifest.initial_state`. Defaults to
    ///   `Value::Option<list<u8>>::None` if neither is provided.
    /// * `response_tx` - Channel to receive the result (actor ID or error)
    /// * `subscription_tx` - Optional channel subscribed to the actor's chain
    ///   BEFORE init, so no event is missed. Callers that want the actor's
    ///   outcome watch here for its terminal lifecycle event (cause + final
    ///   state) — the runtime notifies no one directly.
    ///
    /// Use [`Self::SetupActor`] for the "setup only, do not call init" variant
    /// (the replay path uses this — the replay handler walks the chain and
    /// fires init from the recorded events).
    SpawnActor {
        wasm_bytes: Vec<u8>,
        name: Option<String>,
        manifest: Option<ManifestConfig>,
        init_state: Value,
        response_tx: oneshot::Sender<std::result::Result<TheaterId, crate::errors::SpawnError>>,
        subscription_tx: Option<Sender<(TheaterId, ChainEvent)>>,
        /// Id of the actor that spawned this one, forwarded to the runtime-wide
        /// spawn feed as birth telemetry. `None` for top-level actors. The
        /// runtime retains no lineage of its own (supervision lives in the
        /// supervisor handler).
        parent_id: Option<TheaterId>,
    },

    /// # Setup a new actor (setup only, no init)
    ///
    /// Same as [`Self::SpawnActor`] but does NOT call the actor's `init`
    /// export — task loops are up, handlers attach, the actor can receive
    /// RPCs, but its startup logic hasn't fired. The caller (or a handler
    /// like `ReplayHandler`) is responsible for triggering init when ready.
    ///
    /// ## Parameters
    ///
    /// Identical to `SpawnActor`. The `init_state` is still stored as the
    /// actor's initial state — it just isn't consumed by an automatic init
    /// call. When a caller later RPCs `actor.init`, the actor receives that
    /// state as the first parameter.
    SetupActor {
        wasm_bytes: Vec<u8>,
        name: Option<String>,
        manifest: Option<ManifestConfig>,
        init_state: Value,
        response_tx: oneshot::Sender<std::result::Result<TheaterId, crate::errors::SpawnError>>,
        subscription_tx: Option<Sender<(TheaterId, ChainEvent)>>,
        /// See [`Self::SpawnActor::parent_id`] — birth telemetry only.
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

    /// # Terminate an actor
    ///
    /// Forcefully terminates an actor and cleans up its resources.
    /// This will abort any ongoing operations and remove the actor from the system.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to terminate
    /// * `response_tx` - Channel to receive the result (success or error)
    TerminateActor {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<()>>,
    },

    ShuttingDown {
        actor_id: TheaterId,
        data: Option<Vec<u8>>,
    },

    /// # Actor shutdown complete
    ///
    /// Sent by the spawned shutdown task after the actor runtime has
    /// acknowledged shutdown. Triggers final cleanup (removing from maps,
    /// signaling handler shutdown, etc.)
    ActorShutdownComplete {
        actor_id: TheaterId,
    },

    /// # Shutdown the entire runtime
    ///
    /// Gracefully shuts down the theater runtime and all actors.
    /// The runtime will stop all actors, clean up resources, and exit.
    ShutdownRuntime,

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

    ActorRuntimeError {
        error: ActorRuntimeError,
    },

    /// # Get all actors
    ///
    /// Retrieves a list of all actor IDs in the system.
    ///
    /// ## Parameters
    ///
    /// * `response_tx` - Channel to receive the result (list of actor IDs)
    GetActors {
        /// Returns `(actor-id, name, parent-id)` for every live actor. The
        /// runtime holds no lineage, so `parent-id` is always `None`; the
        /// supervisor fills a child's parent (itself) for scoped views.
        response_tx: oneshot::Sender<Result<Vec<ActorTreeRow>>>,
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
        response_tx: oneshot::Sender<Result<Value>>,
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
        event_tx: Sender<(TheaterId, ChainEvent)>,
    },

    /// # Unsubscribe a previously-registered chain subscriber
    ///
    /// Removes the subscriber that points at `event_tx` from `actor_id`'s
    /// chain. Identity is by `tokio::sync::mpsc::Sender::same_channel` —
    /// the sender passed here must be a clone of the same Sender originally
    /// registered via `SubscribeToActor` (or via `SpawnActor`'s
    /// `subscription_tx`). A no-op if the chain has no matching subscriber
    /// or the actor is no longer running.
    UnsubscribeFromActor {
        actor_id: TheaterId,
        event_tx: Sender<(TheaterId, ChainEvent)>,
    },

    /// # A fate-linked peer terminated — stop the linking actor
    ///
    /// Sent by an actor's `lifecycle` handler when it matches a `StopSelf`
    /// (link) subscription against `peer`'s terminal event: stop `actor_id`
    /// with cause `PeerKilled { peer }`. This is the handler-driven fate
    /// cascade — the runtime holds no relationships itself.
    PeerTerminated {
        actor_id: TheaterId,
        peer: TheaterId,
    },

    /// # Subscribe to actor-spawned notifications (runtime-wide)
    ///
    /// After this call, every actor spawned ANYWHERE in the runtime is
    /// delivered to `event_tx` as `(id, name, parent-id)`. Births only —
    /// deaths ride each actor's own chain subscription (`SubscribeToActor`).
    /// This backs `theater:simple/runtime.subscribe-to-spawns`.
    SubscribeToSpawns {
        event_tx: Sender<ActorTreeRow>,
    },

    /// # Unsubscribe a previously-registered spawn subscriber
    ///
    /// Identity is by `Sender::same_channel` (pass a clone of the sender used
    /// to subscribe). No-op if not subscribed.
    UnsubscribeFromSpawns {
        event_tx: Sender<ActorTreeRow>,
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

    /// # Get an actor handle
    ///
    /// Retrieves a handle for an actor, allowing direct function calls.
    /// Used by the RPC handler for actor-to-actor communication.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to get a handle for
    /// * `response_tx` - Channel to receive the handle (or None if actor not found)
    GetActorHandle {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Option<ActorHandle>>,
    },

    /// # Get actor export hashes
    ///
    /// Retrieves the interface hashes for all interfaces exported by an actor.
    /// Used by the RPC handler for interface compatibility checking.
    ///
    /// ## Parameters
    ///
    /// * `actor_id` - ID of the actor to query
    /// * `response_tx` - Channel to receive the export hashes (or None if actor not found)
    GetActorExportHashes {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Option<Vec<InterfaceHash>>>,
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
            TheaterCommand::SpawnActor { name, .. } => {
                format!("SpawnActor: {}", name.as_deref().unwrap_or("<unnamed>"))
            }
            TheaterCommand::SetupActor { name, .. } => {
                format!("SetupActor: {}", name.as_deref().unwrap_or("<unnamed>"))
            }
            TheaterCommand::StopActor { actor_id, .. } => {
                format!("StopActor: {:?}", actor_id)
            }
            TheaterCommand::TerminateActor { actor_id, .. } => {
                format!("TerminateActor: {:?}", actor_id)
            }
            TheaterCommand::ShuttingDown { actor_id, data } => {
                format!(
                    "ShuttingDown: {:?} (data: {:?})",
                    actor_id,
                    data.as_ref().map(|d| String::from_utf8_lossy(d))
                )
            }
            TheaterCommand::ActorShutdownComplete { actor_id } => {
                format!("ActorShutdownComplete: {:?}", actor_id)
            }
            TheaterCommand::ActorError { actor_id, .. } => {
                format!("ActorError: {:?}", actor_id)
            }
            TheaterCommand::ActorRuntimeError { .. } => "ActorRuntimeError".to_string(),
            TheaterCommand::GetActors { .. } => "GetActors".to_string(),
            TheaterCommand::GetActorManifest { actor_id, .. } => {
                format!("GetActorManifest: {:?}", actor_id)
            }
            TheaterCommand::GetActorStatus { actor_id, .. } => {
                format!("GetActorStatus: {:?}", actor_id)
            }
            TheaterCommand::RestartActor { actor_id, .. } => {
                format!("RestartActor: {:?}", actor_id)
            }
            TheaterCommand::GetActorState { actor_id, .. } => {
                format!("GetActorState: {:?}", actor_id)
            }
            TheaterCommand::GetActorMetrics { actor_id, .. } => {
                format!("GetActorMetrics: {:?}", actor_id)
            }
            TheaterCommand::SubscribeToActor { actor_id, .. } => {
                format!("SubscribeToActor: {:?}", actor_id)
            }
            TheaterCommand::UnsubscribeFromActor { actor_id, .. } => {
                format!("UnsubscribeFromActor: {:?}", actor_id)
            }
            TheaterCommand::PeerTerminated { actor_id, peer } => {
                format!("PeerTerminated: {:?} (peer {:?})", actor_id, peer)
            }
            TheaterCommand::SubscribeToSpawns { .. } => "SubscribeToSpawns".to_string(),
            TheaterCommand::UnsubscribeFromSpawns { .. } => "UnsubscribeFromSpawns".to_string(),
            TheaterCommand::NewStore { .. } => "NewStore".to_string(),
            TheaterCommand::GetActorHandle { actor_id, .. } => {
                format!("GetActorHandle: {:?}", actor_id)
            }
            TheaterCommand::GetActorExportHashes { actor_id, .. } => {
                format!("GetActorExportHashes: {:?}", actor_id)
            }
            TheaterCommand::ShutdownRuntime => "ShutdownRuntime".to_string(),
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
/// let actor_id = TheaterId::generate();
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

    /// # Parse a channel ID from a string
    ///
    /// Creates a ChannelId from its string representation.
    ///
    /// ## Parameters
    ///
    /// * `s` - The string to parse (should be in the format "ch_XXXXXXXXXXXXXXXX")
    ///
    /// ## Returns
    ///
    /// * `Ok(ChannelId)` - Successfully parsed channel ID
    /// * `Err` - Invalid format or empty string
    ///
    /// ## Example
    ///
    /// ```rust
    /// use theater::messages::ChannelId;
    ///
    /// let channel_id = ChannelId::parse("ch_0123456789abcdef").unwrap();
    /// assert_eq!(channel_id.as_str(), "ch_0123456789abcdef");
    /// ```
    pub fn parse(s: &str) -> Result<Self> {
        if s.is_empty() {
            anyhow::bail!("Channel ID cannot be empty");
        }
        if !s.starts_with("ch_") {
            anyhow::bail!("Channel ID must start with 'ch_' prefix");
        }
        Ok(ChannelId(s.to_string()))
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
/// let actor_id = TheaterId::generate();
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
    /// The participant initiating the channel
    pub initiator_id: ChannelParticipant,
    /// Channel to receive the result of the open request
    pub response_tx: oneshot::Sender<Result<bool>>,
    /// Initial message data (may contain authentication/metadata)
    pub initial_msg: Vec<u8>,
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
    pub msg: Vec<u8>,
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

/// # Message Command
///
/// Commands for the message-server handler's messaging infrastructure.
///
/// ## Purpose
///
/// MessageCommand provides a separate command space from TheaterCommand specifically
/// for actor-to-actor messaging operations. This separation allows the message-server
/// handler to manage messaging independently from the core runtime.
///
/// ## Design
///
/// MessageCommand enables complete architectural separation:
/// - External MessageRouter manages actor registry
/// - Message routing is handled externally from the runtime
/// - Handlers register themselves during setup_host_functions
///
/// ## Integration
///
/// Actor WASM host functions send MessageCommands to route messages:
/// - send() → MessageCommand::SendMessage
/// - request() → MessageCommand::SendMessage (with Request type)
/// - open-channel() → MessageCommand::OpenChannel
/// - send-on-channel() → MessageCommand::ChannelMessage
/// - close-channel() → MessageCommand::ChannelClose
#[derive(Debug)]
pub enum MessageCommand {
    /// Send a one-way message to an actor
    ///
    /// Delivers a message to the target actor's mailbox without waiting for a response.
    SendMessage {
        target_id: TheaterId,
        message: ActorMessage,
        response_tx: oneshot::Sender<Result<()>>,
    },

    /// Open a bidirectional channel between actors
    ///
    /// Initiates a channel creation between two participants. The target actor
    /// receives a ChannelOpen message and can accept or reject the channel.
    OpenChannel {
        initiator_id: ChannelParticipant,
        target_id: ChannelParticipant,
        channel_id: ChannelId,
        initial_message: Vec<u8>,
        response_tx: oneshot::Sender<Result<bool>>,
    },

    /// Send a message on an established channel
    ///
    /// Transmits data over an existing channel to the other participant.
    ChannelMessage {
        channel_id: ChannelId,
        sender_id: ChannelParticipant,
        message: Vec<u8>,
        response_tx: oneshot::Sender<Result<()>>,
    },

    /// Close a channel
    ///
    /// Terminates a channel, notifying both participants.
    ChannelClose {
        channel_id: ChannelId,
        sender_id: ChannelParticipant,
        response_tx: oneshot::Sender<Result<()>>,
    },
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActorStatus {
    /// Actor is active and processing messages.
    Running,
    /// Teardown has been initiated. Carries *why* — the terminal cause the
    /// runtime stamps on the actor's final lifecycle event when it completes.
    /// Also the idempotency guard: a second stop while already `Stopping` is a
    /// no-op, so an actor reached by two death paths emits one terminal.
    /// (A fully-torn-down actor is deregistered, so it leaves the map rather
    /// than holding a terminal status.)
    Stopping(crate::events::lifecycle::TerminationCause),
}
