use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::WebSocketServerHandlerConfig;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use axum::{
    extract::ws::{self, Message},
    extract::State,
    extract::WebSocketUpgrade,
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum MessageType {
    #[component(name = "text")]
    Text,
    #[component(name = "binary")]
    Binary,
    #[component(name = "connect")]
    Connect,
    #[component(name = "close")]
    Close,
    #[component(name = "ping")]
    Ping,
    #[component(name = "pong")]
    Pong,
    #[component(name = "other")]
    Other(String),
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketMessage {
    ty: MessageType,
    data: Option<Vec<u8>>,
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct WebSocketResponse {
    messages: Vec<WebSocketMessage>,
}

#[derive(Error, Debug)]
pub enum WebSocketError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Message processing error: {0}")]
    ProcessingError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

struct IncomingMessage {
    connection_id: u64,
    content: Message,
}

struct ConnectionContext {
    sender: Arc<
        Mutex<futures::stream::SplitSink<axum::extract::ws::WebSocket, axum::extract::ws::Message>>,
    >,
}

pub struct WebSocketServerHost {
    port: u16,
    connections: Arc<RwLock<HashMap<u64, ConnectionContext>>>,
}

impl WebSocketServerHost {
    pub fn new(config: WebSocketServerHandlerConfig) -> Self {
        Self {
            port: config.port,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn setup_host_functions(&self, _actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up websocket server host functions");
        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance.register_function::<(WebSocketMessage,), (WebSocketResponse,)>(
            "ntwk:theater/websocket-server",
            "handle-message",
        )
    }

    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        let (message_sender, mut message_receiver) = mpsc::channel(100);

        let app = Router::new()
            .route("/", get(Self::handle_websocket_upgrade))
            .with_state(Arc::new((message_sender.clone(), self.connections.clone())));

        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));
        info!("Starting websocket server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        // Create a shutdown signal for the server
        let (tx, rx) = tokio::sync::oneshot::channel();
        let connections = self.connections.clone();

        // Start the server with graceful shutdown
        let server_task = tokio::spawn(async move {
            let server = axum::serve(listener, app.into_make_service());
            server
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                    debug!("WebSocket server received shutdown signal, starting graceful shutdown");
                })
                .await
        });

        // Message processing task
        let connections_clone = connections.clone();
        let message_task = tokio::spawn(async move {
            while let Some(msg) = message_receiver.recv().await {
                debug!("WebSocket message received");
                if let Err(e) = Self::process_message(msg, &actor_handle, &connections_clone).await
                {
                    error!("Error processing message: {}", e);
                }
                debug!("WebSocket message processed");
            }
        });

        // Now wait for shutdown signal
        tokio::select! {
            // Monitor shutdown channel
            _ = shutdown_receiver.wait_for_shutdown() => {
                debug!("WebSocket server on port {} received shutdown signal", self.port);

                // Send shutdown to server
                let _ = tx.send(());

                // Wait a moment for graceful shutdown
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                // Clean up tasks
                if !server_task.is_finished() {
                    debug!("Aborting WebSocket server task");
                    server_task.abort();
                }

                if !message_task.is_finished() {
                    debug!("Aborting message processing task");
                    message_task.abort();
                }

                debug!("WebSocket server on port {} shutdown complete", self.port);
            }
            // If tasks complete on their own, that's fine too
            _ = server_task => {
                debug!("WebSocket server task completed");
            }
            _ = message_task => {
                debug!("WebSocket message task completed");
            }
        }

        info!("WebSocket server on port {} stopped", self.port);
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
        socket: ws::WebSocket,
        connection_id: u64,
        message_sender: mpsc::Sender<IncomingMessage>,
        connections: Arc<RwLock<HashMap<u64, ConnectionContext>>>,
    ) -> Result<()> {
        info!("New WebSocket connection: {}", connection_id);
        let (sender, mut receiver) = socket.split();

        // Store sender for responses
        connections.write().await.insert(
            connection_id,
            ConnectionContext {
                sender: Arc::new(Mutex::new(sender)),
            },
        );

        // Send initial connection message
        let connect_msg = serde_json::json!({
            "type": "connect",
            "connection_id": connection_id
        })
        .to_string();

        message_sender
            .send(IncomingMessage {
                connection_id,
                content: Message::Text(connect_msg.into()),
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
    ) -> Result<(), WebSocketError> {
        // Convert incoming message to component message
        let component_msg = WebSocketMessage {
            ty: match msg.content {
                Message::Text(_) => MessageType::Text,
                Message::Binary(_) => MessageType::Binary,
                Message::Close(_) => MessageType::Close,
                Message::Ping(_) => MessageType::Ping,
                Message::Pong(_) => MessageType::Pong,
            },
            data: match msg.content {
                Message::Binary(ref b) => Some(b.to_vec()),
                _ => None,
            },
            text: match msg.content {
                Message::Text(ref t) => Some(t.to_string()),
                _ => None,
            },
        };

        let raw_response = actor_handle
            .call_function::<(WebSocketMessage,), (WebSocketResponse,)>(
                "ntwk:theater/websocket-server.handle-message".to_string(),
                (component_msg,),
            )
            .await?;

        // Deserialize response
        let response = raw_response.0;

        // Send responses
        if let Some(connection) = connections.read().await.get(&msg.connection_id) {
            for response_msg in response.messages {
                let ws_msg = match response_msg.ty {
                    MessageType::Text => response_msg.text.map(|t| Message::Text(t.into())),
                    MessageType::Binary => response_msg.data.map(|d| Message::Binary(d.into())),
                    MessageType::Close => Some(Message::Close(None)),
                    MessageType::Ping => Some(Message::Ping(Vec::new().into())),
                    MessageType::Pong => Some(Message::Pong(Vec::new().into())),
                    _ => None,
                };

                if let Some(msg) = ws_msg {
                    if let Err(e) = connection.sender.lock().await.send(msg).await {
                        error!("Error sending response: {}", e);
                    }
                }
            }
        }

        Ok(())
    }
}
