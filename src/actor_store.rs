use crate::chain::{ChainEvent, StateChain};
use crate::id::TheaterId;
use crate::messages::TheaterCommand;
use crate::store::ContentStore;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

/// Store type for sharing resources with WASM host functions
#[derive(Clone)]
pub struct ActorStore {
    pub id: TheaterId,
    pub theater_tx: Sender<TheaterCommand>,
    pub chain: Arc<Mutex<StateChain>>,
    pub state: Option<Vec<u8>>,
    pub content_store: ContentStore,
}

impl ActorStore {
    pub fn new(
        id: TheaterId,
        theater_tx: Sender<TheaterCommand>,
        content_store: ContentStore,
    ) -> Self {
        Self {
            id: id.clone(),
            theater_tx,
            chain: Arc::new(Mutex::new(StateChain::new(id, content_store.clone()))),
            state: Some(vec![]),
            content_store,
        }
    }

    pub fn get_id(&self) -> TheaterId {
        self.id.clone()
    }

    pub fn get_theater_tx(&self) -> Sender<TheaterCommand> {
        self.theater_tx.clone()
    }

    pub fn get_state(&self) -> Option<Vec<u8>> {
        self.state.clone()
    }

    pub fn set_state(&mut self, state: Option<Vec<u8>>) {
        self.state = state;
    }

    pub fn record_event(&self, event_type: String, data: Vec<u8>) -> ChainEvent {
        let mut chain = self.chain.lock().unwrap();
        chain.add_event(event_type, data)
    }

    pub fn verify_chain(&self) -> bool {
        let chain = self.chain.lock().unwrap();
        chain.verify()
    }

    pub fn get_last_event(&self) -> Option<ChainEvent> {
        let chain = self.chain.lock().unwrap();
        chain.get_last_event().cloned()
    }

    pub fn get_all_events(&self) -> Vec<ChainEvent> {
        let chain = self.chain.lock().unwrap();
        chain.get_events().to_vec()
    }

    pub fn get_chain(&self) -> Vec<ChainEvent> {
        self.get_all_events()
    }

    pub fn save_chain(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let chain = self.chain.lock().unwrap();
        chain.save_to_file(path)?;
        Ok(())
    }
}
