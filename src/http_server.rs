use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tide::listener::Listener;
use tide::{Body, Request, Response, Server};
use tokio::sync::{mpsc, oneshot};

use crate::{ActorInput, ActorMessage, ActorOutput, HostHandler};

#[derive(Clone)]
pub struct HttpServerHost {
    mailbox_tx: mpsc::Sender<ActorMessage>,
}

impl HttpServerHost {
    pub fn new(mailbox_tx: mpsc::Sender<ActorMessage>) -> Self {
        Self { mailbox_tx }
    }

    // Handle incoming HTTP request from external clients
    async fn handle_request(mut req: Request<HttpServerHost>) -> tide::Result {
        info!("Received {} request to {}", req.method(), req.url().path());
        // Create a channel for receiving the response
        let (response_tx, response_rx) = oneshot::channel();

        // Get the body bytes
        let body_bytes = req.body_bytes().await?.to_vec();

        // Create actor message with response channel
        let msg = ActorMessage {
            content: ActorInput::HttpRequest {
                method: req.method().to_string(),
                uri: req.url().path().to_string(),
                headers: req
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body: Some(body_bytes),
            },
            response_channel: Some(response_tx),
        };

        // Send to actor
        req.state()
            .mailbox_tx
            .send(msg)
            .await
            .map_err(|_| tide::Error::from_str(500, "Failed to forward request to actor"))?;

        // Wait for response
        let actor_response = response_rx
            .await
            .map_err(|_| tide::Error::from_str(500, "Failed to receive response from actor"))?;

        // Process actor response

        // Parse actor response
        match actor_response {
            ActorOutput::HttpResponse {
                status,
                headers,
                body,
            } => {
                let mut response = Response::new(status);

                // Add headers
                for (key, value) in headers {
                    response.append_header(key.as_str(), value.as_str());
                }

                // Set body if present
                if let Some(body_bytes) = body {
                    // Set response body
                    response.set_body(Body::from_bytes(body_bytes));
                }

                Ok(response)
            }
            _ => Ok(Response::new(500)),
        }
    }
}

pub struct HttpServerHandler {
    port: u16,
}

impl HttpServerHandler {
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

impl HostHandler for HttpServerHandler {
    fn name(&self) -> &str {
        "Http-server"
    }

    fn new(config: Value) -> Self {
        let port = config.get("port").unwrap().as_u64().unwrap() as u16;
        Self { port }
    }

    fn start(
        &self,
        mailbox_tx: mpsc::Sender<ActorMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let state = HttpServerHost::new(mailbox_tx);
            let mut app = Server::with_state(state);
            app.at("/*").all(HttpServerHost::handle_request);
            app.at("/").all(HttpServerHost::handle_request);

            // First bind the server
            let mut listener = app.bind(format!("127.0.0.1:{}", self.port)).await?;

            // Then start accepting connections
            listener.accept().await?;

            info!("HTTP server started on port {}", self.port);

            Ok(())
        })
    }

    fn stop(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async move {
            // Basic shutdown - the server will stop when dropped
            Ok(())
        })
    }
}
