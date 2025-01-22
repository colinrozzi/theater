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
                let handle_message_export =
                    actor.find_export("ntwk:theater/websocket-server", "handle-message")?;

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
            .route("/ws", get(Self::handle_websocket_upgrade))
            .with_state(Arc::new(self.actor_handle.clone()));

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Starting websocket server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Listening on {}", addr);
        axum::serve(listener, app.into_make_service()).await?;
        Ok(())
    }

    // Handle the initial WebSocket upgrade
    async fn handle_websocket_upgrade(
        State(actor_handle): State<Arc<ActorHandle>>,
        ws: WebSocketUpgrade,
    ) -> Response {
        ws.on_upgrade(|socket| async move {
            Self::handle_websocket_connection(socket, actor_handle).await
        })
    }

    // Handle the actual WebSocket connection after upgrade
    async fn handle_websocket_connection(
        socket: axum::extract::ws::WebSocket,
        actor_handle: Arc<ActorHandle>,
    ) {
        let (mut sender, mut receiver) = socket.split();

        // Send a connection message through the normal message handling path
        let mut actor = actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();
        let connect_msg = WebSocketMessage {
            message_type: "text".to_string(),
            data: None,
            text: Some(
                serde_json::json!({
                    "type": "connect"
                })
                .to_string(),
            ),
        };

        if let Ok(((new_state, response),)) = actor
            .call_func::<(WebSocketMessage, Vec<u8>), ((Vec<u8>, WebSocketResponse),)>(
                "handle-message",
                (connect_msg, actor_state),
            )
            .await
        {
            actor.actor_state = new_state;

            // Send any response messages
            for msg in response.messages {
                if let Some(text) = msg.text {
                    let _ = sender
                        .send(axum::extract::ws::Message::Text(text.into()))
                        .await;
                }
            }
        }
        drop(actor);

        // Handle ongoing messages
        while let Some(Ok(msg)) = receiver.next().await {
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
                drop(actor);

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
                            let _ = sender.send(axum::extract::ws::Message::Close(None)).await;
                            return;
                        }
                        _ => {} // Handle other message types as needed
                    }
                }
            }
        }
    }
}

