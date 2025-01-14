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
}
