use crate::actor_handle::ActorHandle;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::wasm::Json;
use crate::wasm::{ActorState, WasmActor};
use crate::ActorStore;
use anyhow::Result;
use axum::{extract::State, response::Response, routing::any, serve, Router};
use serde_json::json;
use serde_json::Value;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;

pub struct MessageServerHost {
    mailbox_rx: Receiver<ActorMessage>,
    theater_tx: Sender<TheaterCommand>,
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
    pub fn new(
        mailbox_rx: Receiver<ActorMessage>,
        theater_tx: Sender<TheaterCommand>,
        actor_handle: ActorHandle,
    ) -> Self {
        Self {
            mailbox_rx,
            theater_tx,
            actor_handle,
        }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for message-server-host");
        let mut actor = self.actor_handle.inner().lock().await;
        let mut interface = actor
            .linker
            .instance("ntwk:theater/message-server-host")
            .expect("could not instantiate ntwk:theater/message-server-host");

        interface
            .func_wrap_async(
                "send",
                |_ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                 (address, msg): (String, Vec<u8>)|
                 -> Box<dyn Future<Output = Result<(Vec<u8>,)>> + Send> {
                    let address = address.clone();
                    let msg = msg.clone();
                    Box::new(async move {
                        let client = reqwest::Client::new();
                        let response = client.post(&address).body(msg).send().await?;
                        let body = response.bytes().await?;
                        Ok((body.to_vec(),))
                    })
                },
            )
            .expect("Failed to wrap async send function");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports for message-server-client");
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
        req: axum::http::Request<axum::body::Body>,
    ) -> Response {
        info!("Received request");

        let (_parts, body) = req.into_parts();
        let bytes = axum::body::to_bytes(body, 100 * 1024 * 1024)
            .await
            .unwrap_or_default();

        let response = Response::builder();

        match serde_json::from_slice::<Value>(&bytes) {
            Ok(val) => {
                info!("Received val: {:?}", val);
                let mut actor = actor_handle.inner().lock().await;
                let actor_state = actor.actor_state.clone();
                match actor
                    .call_func::<(Json, ActorState), ((Json, ActorState),)>(
                        "handle",
                        (
                            serde_json::to_vec(&val).expect("cannot parse val in bytes"),
                            actor_state,
                        ),
                    )
                    .await
                {
                    Ok(((resp, new_state),)) => {
                        actor.actor_state = new_state;
                        info!("success");
                        response
                            .status(200)
                            .body(axum::body::Body::from(resp))
                            .unwrap()
                    }
                    Err(e) => {
                        info!("{}", format!("Error calling handle function: {}", e));
                        response
                            .status(500)
                            .body(axum::body::Body::from(
                                serde_json::to_vec(&json!({
                                    "error": format!("Error calling handle function: {}", e)
                                }))
                                .unwrap(),
                            ))
                            .expect("Failed to set response body")
                    }
                }
            }
            Err(e) => {
                info!("{}", format!("Error parsing request: {}", e));
                response
                    .status(400)
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&json!({
                            "error": format!("Error parsing request: {}", e)
                        }))
                        .unwrap(),
                    ))
                    .expect("Failed to set response body")
            }
        }
    }
}
