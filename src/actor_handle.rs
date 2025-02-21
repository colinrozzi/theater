use crate::wasm::WasmActor;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ActorHandle {
    actor: Arc<Mutex<WasmActor>>,
}

impl ActorHandle {
    pub fn new(actor: WasmActor) -> Self {
        Self {
            actor: Arc::new(Mutex::new(actor)),
        }
    }

    pub fn inner(&self) -> &Arc<Mutex<WasmActor>> {
        &self.actor
    }

    pub async fn with_actor_mut<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&mut WasmActor) -> T,
    {
        let mut actor = self.actor.lock().await;
        f(&mut actor)
    }

    pub async fn get_current_state(&self) -> Option<Vec<u8>> {
        let actor = self.actor.lock().await;
        actor.store.get_current_state()
    }

    pub async fn get_event_history(&self) -> Vec<crate::chain::ChainEvent> {
        let actor = self.actor.lock().await;
        actor.store.get_all_events()
    }
}
