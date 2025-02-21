use crate::actor_handle::ActorHandle;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, ActorRequest, ActorSend, TheaterCommand};
use crate::wasm::Json;
use crate::wasm::{ActorState, WasmActor};
use crate::ActorStore;
use crate::host::host_wrapper::HostFunctionBoundary;
use anyhow::Result;
use std::future::Future;
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;

// ... [rest of the imports and struct definitions]

impl MessageServerHost {
    // ... [previous implementation until handle_send]

    async fn handle_send(&self, data: Vec<u8>) -> () {
        let mut actor = self.actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();

        match actor
            .call_func::<(Json, ActorState), (ActorState,)>(
                "handle-send",
                (Json::from(data), actor_state),
            )
            .await
        {
            Ok((new_state,)) => {
                actor.actor_state = new_state;
            }
            Err(e) => info!("Error processing message: {}", e),
        }
    }

    async fn handle_request(
        &self,
        data: Vec<u8>,
        response_tx: tokio::sync::oneshot::Sender<Vec<u8>>,
    ) -> () {
        let mut actor = self.actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();

        match actor
            .call_func::<(Json, ActorState), ((Json, ActorState),)>(
                "handle-request",
                (Json::from(data), actor_state),
            )
            .await
        {
            Ok(((resp, new_state),)) => {
                actor.actor_state = new_state;
                let _ = response_tx.send(resp.into());
            }
            Err(e) => info!("Error processing message: {}", e),
        }
    }
}