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
}

#[derive(Debug)]
pub enum ActorStatus {
    Running,
    Stopped,
    Failed,
}
