use crate::actor_process::ActorMessage;
use crate::actor_runtime::ChainRequest;
use crate::http_server::HttpServerHost;
use crate::message_server::MessageServerHost;
use tokio::sync::mpsc::Sender;
use tracing::info;

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
