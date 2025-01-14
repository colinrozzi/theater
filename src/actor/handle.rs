use super::wasm::WasmActor;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use wasmtime::component::{ComponentNamedList, Lift, Lower};

use super::command::ActorCommand;

#[derive(Debug, Clone)]
pub struct ActorHandle {
    command_tx: mpsc::Sender<ActorMessage<T, U>>,
}

impl ActorHandle {
    pub fn new(command_tx: mpsc::Sender<ActorMessage<T, U>>) -> Self {
        Self { command_tx }
    }

    pub async fn call_func<T, U>(&self, export_name: String, params: T) -> Result<U>
    where
        T: Lower + ComponentNamedList + Send + Sync + Serialize + Debug + 'static,
        U: Lift + ComponentNamedList + Send + Sync + Serialize + Debug + Clone + 'static,
    {
        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx
            .send(ActorCommand::Call {
                export_name,
                params,
                response_tx,
            })
            .await?;

        response_rx.await?
    }

    pub async fn shutdown(self) -> Result<()> {
        drop(self.command_tx);
        Ok(())
    }
}
