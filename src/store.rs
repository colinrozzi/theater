use crate::chain::ChainRequest;
use crate::messages::TheaterCommand;
use tokio::sync::mpsc::Sender;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct Store {
    pub id: String,
    pub chain_tx: Sender<ChainRequest>,
    pub theater_tx: Sender<TheaterCommand>,
}

impl Store {
    pub fn new(
        id: String,
        chain_tx: Sender<ChainRequest>,
        theater_tx: Sender<TheaterCommand>,
    ) -> Self {
        Self {
            id,
            chain_tx,
            theater_tx,
        }
    }
}
