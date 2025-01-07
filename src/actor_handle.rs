use crate::wasm::WasmActor;
use anyhow::Result;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ActorHandle {
    actor: Mutex<WasmActor>,
}

impl ActorHandle {
    pub fn new(actor: WasmActor) -> Self {
        Self {
            actor: Mutex::new(actor),
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
