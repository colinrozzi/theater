use crate::wasm::WasmActor;
use anyhow::Result;
use std::future::Future;
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

    pub async fn with_actor_future<F, Fut, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&WasmActor) -> Result<Fut>,
        Fut: Future<Output = Result<R>>,
    {
        let actor = self.actor.lock().await;
        let future = f(&actor)?;
        future.await
    }

    pub async fn with_actor_owned<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(WasmActor) -> Result<R>,
    {
        let actor = self.actor.lock().await;
        let actor_clone = actor.clone();
        f(actor_clone)
    }
}
