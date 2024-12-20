use crate::actor_runtime::ChainRequest;
use tokio::sync::mpsc::Sender;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct Store {
    pub chain_tx: Sender<ChainRequest>,
}

impl Store {
    pub fn new(chain_tx: Sender<ChainRequest>) -> Self {
        Self { chain_tx }
    }
}
