use crate::chain::ChainEvent;
use crate::id::TheaterId;
use crate::metrics::ActorMetrics;
use crate::store::ContentStore;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum TheaterCommand {
    SpawnActor {
        manifest_path: String,
        init_bytes: Option<Vec<u8>>,
        response_tx: oneshot::Sender<Result<TheaterId>>,
        parent_id: Option<TheaterId>,
    },
    ResumeActor {
        manifest_path: String,
        state_bytes: Option<Vec<u8>>,
        response_tx: oneshot::Sender<Result<TheaterId>>,
        parent_id: Option<TheaterId>,
    },
    StopActor {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<()>>,
    },
    SendMessage {
        actor_id: TheaterId,
        actor_message: ActorMessage,
    },
    NewEvent {
        actor_id: TheaterId,
        event: ChainEvent,
    },
    ActorError {
        actor_id: TheaterId,
        event: ChainEvent,
    },
    GetActors {
        response_tx: oneshot::Sender<Result<Vec<TheaterId>>>,
    },
    GetActorStatus {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<ActorStatus>>,
    },
    // New supervisor commands
    ListChildren {
        parent_id: TheaterId,
        response_tx: oneshot::Sender<Vec<TheaterId>>,
    },
    RestartActor {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<()>>,
    },
    GetActorState {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<Option<Vec<u8>>>>,
    },
    GetActorEvents {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>>>,
    },
    GetActorMetrics {
        actor_id: TheaterId,
        response_tx: oneshot::Sender<Result<ActorMetrics>>,
    },
    SubscribeToActor {
        actor_id: TheaterId,
        event_tx: Sender<ChainEvent>,
    },
    // Channel-related commands
    ChannelOpen {
        initiator_id: ChannelParticipant,
        target_id: ChannelParticipant,
        channel_id: ChannelId,
        initial_message: Vec<u8>,
        response_tx: oneshot::Sender<Result<bool>>,
    },
    ChannelMessage {
        channel_id: ChannelId,
        sender_id: ChannelParticipant,
        message: Vec<u8>,
    },
    ChannelClose {
        channel_id: ChannelId,
    },
    // Channel diagnostics
    ListChannels {
        response_tx: oneshot::Sender<Result<Vec<(ChannelId, Vec<ChannelParticipant>)>>>,
    },
    GetChannelStatus {
        channel_id: ChannelId,
        response_tx: oneshot::Sender<Result<Option<Vec<ChannelParticipant>>>>,
    },
    // Internal channel management
    RegisterChannel {
        channel_id: ChannelId,
        participants: Vec<ChannelParticipant>,
    },

    // Store commands
    NewStore {
        response_tx: oneshot::Sender<Result<ContentStore>>,
    },
}

impl TheaterCommand {
    pub fn to_log(&self) -> String {
        match self {
            TheaterCommand::SpawnActor { manifest_path, .. } => {
                format!("SpawnActor: {}", manifest_path)
            }
            TheaterCommand::ResumeActor { manifest_path, .. } => {
                format!("ResumeActor: {}", manifest_path)
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

// Channel ID type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChannelId(pub String);

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ChannelId {
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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

#[derive(Debug)]
pub struct ActorRequest {
    pub response_tx: oneshot::Sender<Vec<u8>>,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ActorSend {
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ActorChannelOpen {
    pub channel_id: ChannelId,
    pub response_tx: oneshot::Sender<Result<bool>>,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ActorChannelMessage {
    pub channel_id: ChannelId,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ActorChannelClose {
    pub channel_id: ChannelId,
}

#[derive(Debug)]
pub struct ActorChannelInitiated {
    pub channel_id: ChannelId,
    pub target_id: ChannelParticipant,
    pub initial_msg: Vec<u8>,
}

#[derive(Debug)]
pub enum ActorMessage {
    Request(ActorRequest),
    Send(ActorSend),
    ChannelOpen(ActorChannelOpen),
    ChannelMessage(ActorChannelMessage),
    ChannelClose(ActorChannelClose),
    ChannelInitiated(ActorChannelInitiated),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorStatus {
    Running,
    Stopped,
    Failed,
}
