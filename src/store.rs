use crate::chain::ChainRequest;
use crate::messages::TheaterCommand;
use tokio::sync::mpsc::Sender;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct Store {
    pub chain_tx: Sender<ChainRequest>,
    pub theater_tx: Sender<TheaterCommand>,
}

impl Store {
    pub fn new(chain_tx: Sender<ChainRequest>, theater_tx: Sender<TheaterCommand>) -> Self {
        Self {
            chain_tx,
            theater_tx,
        }
    }
}
