use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use tokio::sync::mpsc::Sender;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct ActorStore {
    pub id: TheaterId,
    pub theater_tx: Sender<TheaterCommand>,
}

impl ActorStore {
    pub fn new(id: TheaterId, theater_tx: Sender<TheaterCommand>) -> Self {
        Self { id, theater_tx }
    }
}