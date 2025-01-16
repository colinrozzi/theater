use crate::actor_handle::ActorHandle;
use crate::wasm::{ActorState, Event, WasmActor};
use crate::Store;
use anyhow::Result;
use axum::{body::Bytes, extract::State, response::IntoResponse, routing::any, serve, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

#[derive(Clone)]
pub struct MessageServerHost {
    port: u16,
    actor_handle: ActorHandle,
}

#[derive(Error, Debug)]
pub enum MessageServerError {
    #[error("Calling WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },
}

impl MessageServerHost {
    pub fn new(port: u16, actor_handle: ActorHandle) -> Self {
        Self { port, actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for filesystem");
        let mut actor = self.actor_handle.inner().lock().await;
        let mut interface = actor
            .linker
            .instance("ntwk:theater/message-server-host")
            .expect("could not instantiate ntwk:theater/message-server-host");

        interface.func_wrap(
            "send",
            |ctx: wasmtime::StoreContextMut<'_, Store>, (address, msg): (String, Vec<u8>)| {
                info!("Sending message to {}", address);
                let cur_head = ctx.get_chain().head();
                let evt = Event {
                    event_type: "actor-message".to_string(),
                    parent: cur_head,
                    data: msg,
                };

                // Since we're now fully in the Tokio runtime, this should work
                tokio::spawn(async move {
                    let client = reqwest::Client::new();
                    if let Err(e) = client.post(&address).json(&evt).send().await {
                        tracing::error!("Failed to send message: {}", e);
                    }
                });
                info!("Message sent");
                Ok(())
            },
        )?;
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports to http-server");
        let _ = self
            .actor_handle
            .with_actor_mut(|actor: &mut WasmActor| -> Result<()> {
                let handle_export = actor
                    .find_export("ntwk:theater/message-server-client", "handle")
                    .expect("Failed to find handle export");
                actor.exports.insert("handle".to_string(), handle_export);
                Ok(())
            })
            .await;
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let app = Router::new()
            .route("/", any(Self::handle_request))
            .route("/{*path}", any(Self::handle_request))
            .with_state(Arc::new(self.actor_handle.clone()));

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Message server starting on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        serve(listener, app.into_make_service()).await?;

        Ok(())
    }

    async fn handle_request(
        State(actor_handle): State<Arc<ActorHandle>>,
        bytes: Bytes,
    ) -> impl IntoResponse {
        info!("Received request");

        match serde_json::from_slice::<Event>(&bytes) {
            Ok(evt) => {
                info!("Received event: {:?}", evt);
                let mut actor = actor_handle.inner().lock().await;
                match actor
                    .call_func::<(Event, ActorState), (ActorState,)>(
                        "handle",
                        (evt, actor.actor_state.clone()),
                    )
                    .await
                {
                    Ok((new_state,)) => {
                        actor.actor_state = new_state;
                        info!("success");
                        "Request forwarded to actor".into_response()
                    }
                    Err(e) => format!("Error handling request: {}", e).into_response(),
                }
            }
            Err(e) => format!("Error parsing event: {}", e).into_response(),
        }
    }
}
