use crate::actor_handle::ActorHandle;
use crate::config::WebSocketServerHandlerConfig;
use crate::wasm::WasmActor;
use anyhow::Result;
use axum::{
    extract::ws::Message, extract::State, extract::WebSocketUpgrade, response::Response,
    routing::get, Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tracing::{error, info};
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
    actor_handle: ActorHandle,
    connections: Arc<RwLock<HashMap<u64, ConnectionContext>>>,
}

impl WebSocketServerHost {
    pub fn new(config: WebSocketServerHandlerConfig, actor_handle: ActorHandle) -> Self {
        Self {
            port: config.port,
            actor_handle,
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
                let handle_message_export = actor
                    .find_export("ntwk:theater/websocket-server", "handle-message")
                    .expect("Could not find handle-message export");

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
        let (message_sender, mut message_receiver) = mpsc::channel(100);

        let app = Router::new()
            .route("/", get(Self::handle_websocket_upgrade))
            .with_state(Arc::new((message_sender.clone(), self.connections.clone())));

        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));
        info!("Starting websocket server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        // Start message processing
        let actor_handle = self.actor_handle.clone();
        let connections = self.connections.clone();

        tokio::spawn(async move {
            while let Some(msg) = message_receiver.recv().await {
                info!("Message Recieved");
                if let Err(e) = Self::process_message(msg, &actor_handle, &connections).await {
                    error!("Error processing message: {}", e);
                }
                info!("Message processed");
            }
        });

        // Spawn the server task
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app.into_make_service()).await {
                error!("Server error: {}", e);
            }
        });

        info!("Listening on {}", addr);
        //  axum::serve(listener, app.into_make_service()).await?;
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
        connections.write().await.insert(
            connection_id,
            ConnectionContext {
                sender: Arc::new(Mutex::new(sender)),
            },
        );

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
                Message::Text(t) => Some(t.to_string()),
                _ => None,
            },
        };

        // Process with actor
        let mut actor = actor_handle.inner().lock().await;
        info!("Claimed actor handle");
        let actor_state: Vec<u8> = actor.actor_state.clone();
        match actor
            .call_func::<(WebSocketMessage, Vec<u8>), ((Vec<u8>, WebSocketResponse),)>(
                "handle-message",
                (component_msg, actor_state),
            )
            .await
        {
            Ok(((new_state, response),)) => {
                info!("Actor function call successful");
                actor.actor_state = new_state;

                // Send responses
                if let Some(connection) = connections.read().await.get(&msg.connection_id) {
                    for response_msg in response.messages {
                        let ws_msg = match response_msg.ty {
                            MessageType::Text => {
                                response_msg.text.map(|arg0: std::string::String| {
                                    axum::extract::ws::Message::Text(arg0.into())
                                })
                            }
                            MessageType::Binary => response_msg
                                .data
                                .map(|arg0: Vec<u8>| Message::Binary(arg0.into())),
                            MessageType::Close => Some(Message::Close(None)),
                            MessageType::Ping => Some(Message::Ping(vec![].into())),
                            MessageType::Pong => Some(Message::Pong(vec![].into())),
                            MessageType::Connect => None, // Not applicable for outgoing messages
                            MessageType::Other(_) => None, // Handle any other types as needed
                        };

                        if let Some(msg) = ws_msg {
                            if let Err(e) = connection.sender.lock().await.send(msg).await {
                                error!("Error sending response: {}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Actor function call failed: {}", e);
            }
        }

        Ok(())
    }
}
