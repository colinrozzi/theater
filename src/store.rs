use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::chain::{StateChain, ChainEvent};
use tokio::sync::mpsc::Sender;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct ActorStore {
    pub id: TheaterId,
    pub theater_tx: Sender<TheaterCommand>,
    chain: Arc<Mutex<StateChain>>,
}

impl ActorStore {
    pub fn new(id: TheaterId, theater_tx: Sender<TheaterCommand>) -> Self {
        Self { 
            id, 
            theater_tx,
            chain: Arc::new(Mutex::new(StateChain::new())),
        }
    }

    pub async fn record_event(&self, event_type: String, data: Vec<u8>) -> ChainEvent {
        let mut chain = self.chain.lock().await;
        chain.add_event(event_type, data)
    }

    pub async fn verify_chain(&self) -> bool {
        let chain = self.chain.lock().await;
        chain.verify()
    }

    pub async fn get_last_event(&self) -> Option<ChainEvent> {
        let chain = self.chain.lock().await;
        chain.get_last_event().cloned()
    }

    pub async fn save_chain(&self, path: &std::path::Path) -> Result<()> {
        let chain = self.chain.lock().await;
        chain.save_to_file(path)?;
        Ok(())
    }

    pub async fn load_chain(&self, path: &std::path::Path) -> Result<()> {
        let chain = StateChain::load_from_file(path)?;
        let mut current_chain = self.chain.lock().await;
        *current_chain = chain;
        Ok(())
    }
}