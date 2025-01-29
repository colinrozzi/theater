use crate::Result;
use std::path::PathBuf;
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum TheaterCommand {
    SpawnActor {
        manifest_path: PathBuf,
        response_tx: oneshot::Sender<Result<String>>, // Returns actor ID on success
    },
    StopActor {
        actor_id: String,
        response_tx: oneshot::Sender<Result<()>>,
    },
    SendMessage {
        actor_id: String,
        actor_message: ActorMessage,
    },
    NewEvent {
        actor_id: String,
        event: Vec<u8>,
    },
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
