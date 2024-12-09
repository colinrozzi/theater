use anyhow::Result;
use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use axum_macros::debug_handler;

use hyper::StatusCode;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::{ActorInput, ActorMessage, ActorOutput, HostInterface};

pub struct HttpHost {
    port: u16,
    mailbox_tx: Option<mpsc::Sender<ActorMessage>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Msg {
    data: Value,
}

impl HttpHost {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            mailbox_tx: None,
        }
    }

    #[debug_handler]
    async fn handle_request(
        mailbox_tx: &mpsc::Sender<ActorMessage>,
        Json(payload): Json<Value>,
    ) -> Result<Json<Msg>, StatusCode> {
        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Send message to actor
        let msg = ActorMessage {
            content: ActorInput::Message(payload),
            response_channel: Some(tx),
        };

        mailbox_tx
            .send(msg)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        match response {
            ActorOutput::Message(value) => Ok(Json(Msg { data: value })),
            ActorOutput::HttpResponse { .. } => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
}

impl HostInterface for HttpHost {
    async fn start(&mut self, mailbox_tx: mpsc::Sender<ActorMessage>) -> Result<()> {
        self.mailbox_tx = Some(mailbox_tx.clone());

        // Build router
        let app = Router::new().route(
            "/",
            post(HttpHost::handle_request).with_state(mailbox_tx.clone()),
        );

        // Run server
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
        println!("HTTP interface listening on http://{}", addr);

        /*
                axum::Server::bind(&addr)
                    .serve(app.into_make_service())
                    .await?;
        */

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // Server will stop when dropped
        Ok(())
    }
}
/*
pub async fn serve(runtime: Runtime, addr: SocketAddr) -> anyhow::Result<()> {
    let shared = SharedRuntime(Arc::new(Mutex::new(runtime)));

    let app = Router::new()
        .route("/", post(handle_message))
        .route("/chain", get(handle_get_chain))
        .with_state(shared);

    println!("Starting server on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;

    Ok(())
}

async fn handle_message(
    State(shared): State<SharedRuntime>,
    Json(message): Json<Message>,
) -> Result<Json<Response>, (StatusCode, String)> {
    println!("Received message: {:?}", message);
    let mut runtime = shared.0.lock().await;
    match runtime.handle_message(message.data).await {
        Ok((hash, state)) => Ok(Json(Response { hash, state })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn handle_get_chain(State(shared): State<SharedRuntime>) -> Json<ChainResponse> {
    println!("Getting chain");
    let runtime = shared.0.lock().await;
    let chain = runtime.get_chain();

    Json(ChainResponse {
        head: chain.get_head().map(String::from),
        entries: chain.get_full_chain(),
    })
}
*/
