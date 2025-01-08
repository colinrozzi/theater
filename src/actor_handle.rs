use crate::wasm::WasmActor;
use anyhow::Result;
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

    pub async fn with_actor<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&WasmActor) -> Result<R>,
    {
        let actor = self.actor.lock().await;
        f(&actor)
    }
}
