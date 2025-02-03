use crate::id::TheaterId;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::oneshot;
use wasmtime::chain::MetaEvent;

#[derive(Debug)]
pub enum TheaterCommand {
    SpawnActor {
        manifest_path: PathBuf,
        response_tx: oneshot::Sender<Result<TheaterId>>, // Now returns TheaterId instead of String
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
        event: Vec<MetaEvent>,
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
        }
    }
}

#[derive(Debug)]
pub struct ActorMessage {
    pub response_tx: oneshot::Sender<Vec<u8>>,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum ActorStatus {
    Running,
    Stopped,
    Failed,
}
