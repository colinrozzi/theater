use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tide::{Request, Response, Server};
use tokio::sync::mpsc;

use crate::{ActorInput, ActorMessage, HostHandler};

// HTTP interface for actor-to-actor communication
#[derive(Clone)]
#[allow(dead_code)]
pub struct HttpHost {
    client: Client,
    port: u16,
    mailbox_tx: mpsc::Sender<ActorMessage>,
}

impl HttpHost {
    pub fn new(port: u16, mailbox_tx: mpsc::Sender<ActorMessage>) -> Self {
        Self {
            client: Client::new(),
            port,
            mailbox_tx,
        }
    }

    // Send a message to another actor
    pub async fn send_message(&self, address: String, message: Value) -> Result<()> {
        println!("Sending message to {}: {:?}", address, message);
        // Fire and forget POST request
        self.client
            .post(address)
            .json(&message)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send message: {}", e))?;

        Ok(())
    }

    // Handle incoming message
    async fn handle_request(mut req: Request<mpsc::Sender<ActorMessage>>) -> tide::Result {
        match req.method() {
            tide::http::Method::Post => {
                // Get JSON payload
                let payload: Value = req.body_json().await?;

                println!("Received message: {:?}", payload);

                // Create message with no response channel
                let msg = ActorMessage {
                    content: ActorInput::Message(payload),
                    response_channel: None, // One-way message
                };

                // Send to actor
                req.state()
                    .send(msg)
                    .await
                    .map_err(|_| tide::Error::from_str(500, "Failed to forward message"))?;

                // Simple OK response
                Ok(Response::new(200))
            }
            _ => Ok(Response::new(405)), // Method Not Allowed
        }
    }
}

pub struct HttpHandler {
    port: u16,
}

impl HttpHandler {
    pub fn new(port: u16) -> Self {
        Self { port }
    }
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
        mailbox_tx: mpsc::Sender<ActorMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        println!("Starting http actor mailbox on port {}", self.port);
        println!("[HTTP] Attempting to bind to 127.0.0.1:{}", self.port);
        Box::pin(async move {
            let mut app = Server::with_state(mailbox_tx);
            app.at("/").post(HttpHost::handle_request);

            // Create a channel to signal when we're bound
            let (tx, rx) = tokio::sync::oneshot::channel();
            println!("[HTTP] Setting up routes on /");

            // Spawn the server in a separate task
            let server_port = self.port;
            tokio::spawn(async move {
                match app.listen(format!("127.0.0.1:{}", server_port)).await {
                    Ok(_) => {
                        println!("[HTTP] Successfully bound to port {}", server_port);
                        let _ = tx.send(Ok(()));
                    }
                    Err(e) => {
                        println!("[HTTP] Failed to bind to port {}: {}", server_port, e);
                        let _ = tx.send(Err(anyhow!("Failed to bind HTTP server: {}", e)));
                    }
                }
            });

            // Wait for server to bind
            rx.await
                .map_err(|e| anyhow!("Server startup failed: {}", e))?
                .map_err(|e| e)?;

            println!("[HTTP] Server started on port {}", self.port);

            // Keep this task alive
            std::future::pending::<()>().await;

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
