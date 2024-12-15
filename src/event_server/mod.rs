use crate::chain_emitter::CHAIN_EMITTER;
use crate::logging::ChainEvent;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::net::SocketAddr;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

pub async fn run_event_server(port: u16) {
    // Create our router
    let app = Router::new()
        .route("/events/history", get(get_history))
        .route("/events/ws", get(websocket_handler))
        .layer(CorsLayer::permissive()); // Be careful with this in production!

    // Run it
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Event server listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn get_history() -> impl IntoResponse {
    let history = CHAIN_EMITTER.get_history();
    axum::Json(json!({
        "events": history
    }))
}

async fn websocket_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to chain events
    let mut event_rx = CHAIN_EMITTER.subscribe();

    // Spawn a task to forward events to the WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender
                    .send(axum::extract::ws::Message::Text(json))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    // Wait for the client to close or the send task to finish
    tokio::select! {
        _ = (&mut send_task) => {},
        _ = receiver.next() => {
            send_task.abort();
        }
    }
}

