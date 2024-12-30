use crate::actor_process::{ActorMessage, ProcessMessage};
use crate::wasm::Event;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tide::{Body, Request, Response, Server};
use tokio::sync::mpsc;
use tracing::info;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessageServerHost {
    port: u16,
}

impl MessageServerHost {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub async fn start(&self, mailbox_tx: mpsc::Sender<ProcessMessage>) -> Result<()> {
        let mut app = Server::with_state(mailbox_tx.clone());
        app.at("/*").all(Self::handle_request);
        app.at("/").all(Self::handle_request);

        info!("Message server starting on port {}", self.port);
        app.listen(format!("127.0.0.1:{}", self.port)).await?;

        Ok(())
    }

    async fn handle_request(mut req: Request<mpsc::Sender<ProcessMessage>>) -> tide::Result {
        info!("Received {} request to {}", req.method(), req.url().path());

        // Get the body bytes
        let body_bytes = req.body_bytes().await?.to_vec();

        let evt = Event {
            type_: "actor_message".to_string(),
            data: json!(body_bytes),
        };

        let process_msg = ProcessMessage::ActorMessage(ActorMessage {
            event: evt,
            response_channel: None,
        });

        // Send to actor
        req.state()
            .send(process_msg)
            .await
            .map_err(|_| tide::Error::from_str(500, "Failed to forward request to actor"))?;

        Ok(Response::builder(200)
            .body(Body::from_string("Request forwarded to actor".to_string()))
            .build())
    }
}
