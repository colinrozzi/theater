use crate::chain::ChainEvent;
use crate::id::TheaterId;
use crate::metrics::ActorMetrics;
use crate::Result;
use serde::{Deserialize, Serialize};
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
pub enum ActorMessage {
    Request(ActorRequest),
    Send(ActorSend),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorStatus {
    Running,
    Stopped,
    Failed,
}
