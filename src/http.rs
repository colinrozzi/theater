use anyhow::Result;
use serde_json::Value;
use tide::{Body, Request, Response, Server};
use tokio::sync::{mpsc, oneshot};

use crate::{ActorInput, ActorMessage, ActorOutput, HostInterface};

pub struct HttpHost {
    port: u16,
    mailbox_tx: Option<mpsc::Sender<ActorMessage>>,
}

impl HttpHost {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            mailbox_tx: None,
        }
    }

    async fn handle_request(mut req: Request<mpsc::Sender<ActorMessage>>) -> tide::Result {
        // Get JSON payload
        let payload: Value = req.body_json().await?;

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Send message to actor
        let msg = ActorMessage {
            content: ActorInput::Message(payload),
            response_channel: Some(tx),
        };

        req.state()
            .send(msg)
            .await
            .map_err(|_| tide::Error::from_str(500, "Failed to send message"))?;

        // Wait for response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| tide::Error::from_str(408, "Request timeout"))?
            .map_err(|_| tide::Error::from_str(500, "Failed to receive response"))?;

        match response {
            ActorOutput::Message(value) => {
                let mut res = Response::new(200);
                res.set_body(Body::from_json(&value)?);
                Ok(res)
            }
            ActorOutput::HttpResponse { .. } => {
                Err(tide::Error::from_str(500, "Invalid response type"))
            }
        }
    }
}

impl HostInterface for HttpHost {
    async fn start(&mut self, mailbox_tx: mpsc::Sender<ActorMessage>) -> Result<()> {
        self.mailbox_tx = Some(mailbox_tx.clone());

        let mut app = Server::with_state(mailbox_tx);
        app.at("/").post(Self::handle_request);

        let addr = format!("127.0.0.1:{}", self.port);
        println!("HTTP interface listening on http://{}", addr);
        app.listen(addr).await?;

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // Server will stop when dropped
        Ok(())
    }
}

