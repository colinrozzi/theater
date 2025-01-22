use crate::actor_handle::ActorHandle;
use crate::config::WebSocketServerHandlerConfig;
use crate::wasm::WasmActor;
use anyhow::Result;
use axum::{extract::State, extract::WebSocketUpgrade, response::Response, routing::get, Router};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Clone)]
pub struct WebSocketServerHost {
    port: u16,
    actor_handle: ActorHandle,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketMessage {
    message_type: String, // "text", "binary", "ping", "pong", "close"
    data: Option<Vec<u8>>,
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketResponse {
    messages: Vec<WebSocketMessage>,
}

impl WebSocketServerHost {
    pub fn new(config: WebSocketServerHandlerConfig, actor_handle: ActorHandle) -> Self {
        Self {
            port: config.port,
            actor_handle,
        }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports to websocket-server");
        let _ = self
            .actor_handle
            .with_actor_mut(|actor: &mut WasmActor| -> Result<()> {
                let handle_connection_export =
                    actor.find_export("ntwk:theater/websocket-server", "handle-connection")?;
                let handle_message_export =
                    actor.find_export("ntwk:theater/websocket-server", "handle-message")?;

                actor
                    .exports
                    .insert("handle-connection".to_string(), handle_connection_export);
                actor
                    .exports
                    .insert("handle-message".to_string(), handle_message_export);

                info!("Added websocket exports");
                info!("exports: {:?}", actor.exports);
                Ok(())
            })
            .await;
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let app = Router::new()
            .route("/ws", get(Self::handle_websocket))
            .with_state(Arc::new(self.actor_handle.clone()));

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Starting websocket server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Listening on {}", addr);
        axum::serve(listener, app.into_make_service()).await?;
        Ok(())
    }

    async fn handle_websocket(
        State(actor_handle): State<Arc<ActorHandle>>,
        ws: WebSocketUpgrade,
    ) -> Response {
        ws.on_upgrade(|socket| async move {
            let (mut sender, mut receiver) = socket.split();

            // Handle initial connection
            let mut actor = actor_handle.inner().lock().await;
            let actor_state = actor.actor_state.clone();

            if let Ok(((new_state, response),)) = actor
                .call_func::<(Vec<u8>,), ((Vec<u8>, WebSocketResponse),)>(
                    "handle-connection",
                    (actor_state,),
                )
                .await
            {
                actor.actor_state = new_state;

                // Send any initial messages
                for msg in response.messages {
                    match msg.message_type.as_str() {
                        "text" => {
                            if let Some(text) = msg.text {
                                let _ = sender
                                    .send(axum::extract::ws::Message::Text(text.into()))
                                    .await;
                            }
                        }
                        "binary" => {
                            if let Some(data) = msg.data {
                                let _ = sender
                                    .send(axum::extract::ws::Message::Binary(data.into()))
                                    .await;
                            }
                        }
                        _ => {} // Handle other message types as needed
                    }
                }
            }

            // Handle ongoing messages
            while let Some(msg) = receiver.next().await {
                if let Ok(msg) = msg {
                    let websocket_msg = match msg {
                        axum::extract::ws::Message::Text(t) => WebSocketMessage {
                            message_type: "text".to_string(),
                            data: None,
                            text: Some(t.to_string()),
                        },
                        axum::extract::ws::Message::Binary(b) => WebSocketMessage {
                            message_type: "binary".to_string(),
                            data: Some(b.to_vec()),
                            text: None,
                        },
                        axum::extract::ws::Message::Close(_) => WebSocketMessage {
                            message_type: "close".to_string(),
                            data: None,
                            text: None,
                        },
                        axum::extract::ws::Message::Ping(_) => WebSocketMessage {
                            message_type: "ping".to_string(),
                            data: None,
                            text: None,
                        },
                        axum::extract::ws::Message::Pong(_) => WebSocketMessage {
                            message_type: "pong".to_string(),
                            data: None,
                            text: None,
                        },
                    };

                    let mut actor = actor_handle.inner().lock().await;
                    let actor_state = actor.actor_state.clone();

                    if let Ok(((new_state, response),)) = actor
                        .call_func::<(WebSocketMessage, Vec<u8>), ((Vec<u8>, WebSocketResponse),)>(
                            "handle-message",
                            (websocket_msg, actor_state),
                        )
                        .await
                    {
                        actor.actor_state = new_state;

                        // Send response messages
                        for msg in response.messages {
                            match msg.message_type.as_str() {
                                "text" => {
                                    if let Some(text) = msg.text {
                                        let _ = sender
                                            .send(axum::extract::ws::Message::Text(text.into()))
                                            .await;
                                    }
                                }
                                "binary" => {
                                    if let Some(data) = msg.data {
                                        let _ = sender
                                            .send(axum::extract::ws::Message::Binary(data.into()))
                                            .await;
                                    }
                                }
                                "close" => {
                                    let _ =
                                        sender.send(axum::extract::ws::Message::Close(None)).await;
                                    break;
                                }
                                _ => {} // Handle other message types as needed
                            }
                        }
                    }
                }
            }
        })
    }
}

