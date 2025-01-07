use crate::messages::TheaterCommand;
use tokio::sync::mpsc::Sender;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct Store {
    pub id: String,
    pub theater_tx: Sender<TheaterCommand>,
}

impl Store {
    pub fn new(id: String, theater_tx: Sender<TheaterCommand>) -> Self {
        Self { id, theater_tx }
    }
}
