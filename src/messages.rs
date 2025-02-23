use crate::chain::ChainEvent;
use crate::id::TheaterId;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum TheaterCommand {
    SpawnActor {
        manifest_path: PathBuf,
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
    GetChildState {
        child_id: TheaterId,
        response_tx: oneshot::Sender<Result<Vec<u8>>>,
    },
    GetChildEvents {
        child_id: TheaterId,
        response_tx: oneshot::Sender<Result<Vec<ChainEvent>>>,
    },
}

impl TheaterCommand {
    pub fn to_log(&self) -> String {
        match self {
            TheaterCommand::SpawnActor { manifest_path, .. } => {
                format!("SpawnActor: {}", manifest_path.display())
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
            TheaterCommand::GetChildState { child_id, .. } => {
                format!("GetChildState: {:?}", child_id)
            }
            TheaterCommand::GetChildEvents { child_id, .. } => {
                format!("GetChildEvents: {:?}", child_id)
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

#[derive(Debug, Clone)]
pub enum ActorStatus {
    Running,
    Stopped,
    Failed,
}

