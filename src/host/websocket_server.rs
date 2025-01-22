use crate::actor_handle::ActorHandle;
use crate::config::WebSocketServerHandlerConfig;
use crate::wasm::WasmActor;
use anyhow::Result;
use axum::{extract::State, extract::WebSocketUpgrade, response::Response, routing::get, Router};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tracing::{error, info};
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketMessage {
    message_type: String,
    data: Option<Vec<u8>>,
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketResponse {
    messages: Vec<WebSocketMessage>,
}

struct IncomingMessage {
    connection_id: u64,
    content: axum::extract::ws::Message,
}

struct ConnectionContext {
    sender: futures::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>,
}

pub struct WebSocketServerHost {
    port: u16,
    actor_handle: ActorHandle,
    message_sender: mpsc::Sender<IncomingMessage>,
    message_receiver: mpsc::Receiver<IncomingMessage>,
    connections: Arc<RwLock<HashMap<u64, ConnectionContext>>>,
}

impl WebSocketServerHost {
    pub fn new(config: WebSocketServerHandlerConfig, actor_handle: ActorHandle) -> Self {
        let (message_sender, message_receiver) = mpsc::channel(100);
        Self {
            port: config.port,
            actor_handle,
            message_sender,
            message_receiver,
            connections: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn start(&mut self) -> Result<()> {
        let app = Router::new()
            .route("/ws", get(Self::handle_websocket_upgrade))
            .with_state(Arc::new((
                self.message_sender.clone(),
                self.connections.clone(),
            )));

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Starting websocket server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        // Start message processing
        let actor_handle = self.actor_handle.clone();
        let connections = self.connections.clone();

        while let Some(msg) = self.message_receiver.recv().await {
            if let Err(e) = Self::process_message(msg, &actor_handle, &connections).await {
                error!("Error processing message: {}", e);
            }
        }

        info!("Listening on {}", addr);
        axum::serve(listener, app.into_make_service()).await?;
        Ok(())
    }

    async fn handle_websocket_upgrade(
        State(state): State<
            Arc<(
                mpsc::Sender<IncomingMessage>,
                Arc<RwLock<HashMap<u64, ConnectionContext>>>,
            )>,
        >,
        ws: WebSocketUpgrade,
    ) -> Response {
        let (sender, connections) = &*state;
        let connection_id = rand::random::<u64>();
        let sender = sender.clone();
        let connections = connections.clone();

        ws.on_upgrade(move |socket| async move {
            if let Err(e) =
                Self::handle_websocket_connection(socket, connection_id, sender, connections).await
            {
                error!("WebSocket connection error: {}", e);
            }
        })
    }

    async fn handle_websocket_connection(
        socket: axum::extract::ws::WebSocket,
        connection_id: u64,
        message_sender: mpsc::Sender<IncomingMessage>,
        connections: Arc<RwLock<HashMap<u64, ConnectionContext>>>,
    ) -> Result<()> {
        info!("New WebSocket connection: {}", connection_id);
        let (sender, mut receiver) = socket.split();

        // Store sender for responses
        connections
            .write()
            .await
            .insert(connection_id, ConnectionContext { sender });

        // Send initial connection message
        message_sender
            .send(IncomingMessage {
                connection_id,
                content: axum::extract::ws::Message::Text(
                    serde_json::json!({
                        "type": "connect"
                    })
                    .to_string()
                    .into(),
                ),
            })
            .await?;

        // Forward incoming messages
        while let Some(Ok(msg)) = receiver.next().await {
            message_sender
                .send(IncomingMessage {
                    connection_id,
                    content: msg,
                })
                .await?;
        }

        // Clean up on disconnect
        info!("WebSocket disconnected: {}", connection_id);
        connections.write().await.remove(&connection_id);
        Ok(())
    }

    async fn process_message(
        msg: IncomingMessage,
        actor_handle: &ActorHandle,
        connections: &Arc<RwLock<HashMap<u64, ConnectionContext>>>,
    ) -> Result<()> {
        // Convert incoming message to component message
        let component_msg = WebSocketMessage {
            message_type: match msg.content {
                axum::extract::ws::Message::Text(_) => "text".to_string(),
                axum::extract::ws::Message::Binary(_) => "binary".to_string(),
                axum::extract::ws::Message::Close(_) => "close".to_string(),
                axum::extract::ws::Message::Ping(_) => "ping".to_string(),
                axum::extract::ws::Message::Pong(_) => "pong".to_string(),
            },
            data: match msg.content {
                axum::extract::ws::Message::Binary(ref b) => Some(b.to_vec()),
                _ => None,
            },
            text: match msg.content {
                axum::extract::ws::Message::Text(t) => Some(t.to_string()),
                _ => None,
            },
        };

        // Process with actor
        let mut actor = actor_handle.inner().lock().await;
        let actor_state = actor.actor_state.clone();
        if let Ok(((new_state, response),)) = actor
            .call_func::<(WebSocketMessage, Vec<u8>), ((Vec<u8>, WebSocketResponse),)>(
                "handle-message",
                (component_msg, actor_state),
            )
            .await
        {
            actor.actor_state = new_state;
            drop(actor);

            // Send responses
            if let Some(connection) = connections.read().await.get(&msg.connection_id) {
                for response_msg in response.messages {
                    let ws_msg = match response_msg.message_type.as_str() {
                        "text" => response_msg.text.map(|arg0: std::string::String| {
                            axum::extract::ws::Message::Text(arg0.into())
                        }),
                        "binary" => response_msg
                            .data
                            .map(|arg0: Vec<u8>| axum::extract::ws::Message::Binary(arg0.into())),
                        "close" => Some(axum::extract::ws::Message::Close(None)),
                        _ => None,
                    };

                    if let Some(msg) = ws_msg {
                        if let Err(e) = connection.sender.send(msg).await {
                            error!("Error sending response: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
