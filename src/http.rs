use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tide::{Body, Request, Response, Server};
use tokio::sync::{mpsc, oneshot};

use crate::{ActorInput, ActorMessage, ActorOutput, HostHandler};

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

pub struct HttpHandler {
    port: u16,
}

impl HostHandler for HttpHandler {
    fn name(&self) -> &str {
        "http"
    }

    fn new(config: Value) -> Self {
        let port = config.get("port").unwrap().as_u64().unwrap() as u16;
        Self { port }
    }

    fn start(
        &self,
        mailbox: mpsc::Sender<ActorMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut app = Server::with_state(mailbox);
            //        app.at("/").post(handle_http_request);
            // just for testing, return hello world
            app.at("/").get(|_| async { Ok("Hello, world!") });
            app.listen(format!("127.0.0.1:{}", self.port)).await?;
            Ok(())
        })
    }

    fn stop(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async move {
            // Shutdown logic
            Ok(())
        })
    }
}
