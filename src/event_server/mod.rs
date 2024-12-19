use crate::chain_emitter::ChainEvent;
use crate::chain_emitter::CHAIN_EMITTER;
use futures::{SinkExt, StreamExt};
use warp::{ws::Message, Filter};

pub async fn run_event_server(port: u16) {
    // Route for getting event history
    let history = warp::path!("events" / "history").and(warp::get()).map(|| {
        let history = CHAIN_EMITTER.get_history();
        warp::reply::json(&history)
    });

    // Route for WebSocket connections
    let ws = warp::path!("events" / "ws")
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| ws.on_upgrade(handle_ws_client));

    // CORS configuration
    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["content-type"]);

    // Combine routes
    let routes = history.or(ws).with(cors).with(warp::trace::request());

    // Start the server
    println!("Event server listening on port {}", port);
    warp::serve(routes).run(([127, 0, 0, 1], port)).await;
}

async fn handle_ws_client(ws: warp::ws::WebSocket) {
    let (mut ws_tx, mut ws_rx) = ws.split();

    // Subscribe to chain events
    let mut event_rx = CHAIN_EMITTER.subscribe();

    // Forward events to WebSocket clients
    let mut send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if ws_tx.send(Message::text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Keep the connection alive until client disconnects
    tokio::select! {
        _ = (&mut send_task) => {},
        _ = ws_rx.next() => {
            send_task.abort();
        }
    };
}
