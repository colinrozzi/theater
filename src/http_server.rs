use anyhow::{anyhow, Result};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use tide::{Body, Request, Response, Server};
use tokio::sync::{mpsc, oneshot};

use crate::{ActorInput, ActorMessage, ActorOutput, HostHandler};

#[derive(Clone)]
pub struct HttpServerHost {
    port: u16,
    mailbox_tx: mpsc::Sender<ActorMessage>,
}

impl HttpServerHost {
    pub fn new(port: u16, mailbox_tx: mpsc::Sender<ActorMessage>) -> Self {
        Self { port, mailbox_tx }
    }

    // Handle incoming HTTP request from external clients
    async fn handle_request(mut req: Request<HttpServerHost>) -> tide::Result {
        println!(
            "[HTTP_SERVER] Received {} request to {}",
            req.method(),
            req.url().path()
        );
        // Create a channel for receiving the response
        let (response_tx, response_rx) = oneshot::channel();

        // Serialize the HTTP request into the format expected by the actor
        let request_json = serde_json::json!({
            "method": req.method().to_string(),
            "path": req.url().path(),
            "headers": req.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>(),
            "body": String::from_utf8(req.body_bytes().await?.to_vec()).unwrap_or_default()
        });

        // Create actor message with response channel
        let msg = ActorMessage {
            content: ActorInput::Message(request_json),
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

        // Parse actor response
        match actor_response {
            ActorOutput::Message(value) => {
                // The response should be a JSON object with "response" and "state" fields
                let response_obj = value
                    .get("response")
                    .ok_or_else(|| tide::Error::from_str(500, "Invalid response format"))?;

                // Extract HTTP response components
                let status = response_obj
                    .get("status")
                    .and_then(|s| s.as_u64())
                    .unwrap_or(200) as u16;

                let headers = response_obj.get("headers").and_then(|h| h.as_object());

                let body = response_obj
                    .get("body")
                    .and_then(|b| b.as_str())
                    .unwrap_or_default();

                // Build response
                let mut response = Response::new(status);
                headers.map(|h| {
                    for (key, value) in h {
                        if let Some(value_str) = value.as_str() {
                            response.insert_header(key.as_str(), value_str);
                        }
                    }
                });
                /*
                                for (key, value) in headers {
                                    if let Some(value_str) = value.as_str() {
                                        response.insert_header(key, value_str);
                                    }
                                }
                */
                response.set_body(body);

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
        println!(
            "[HTTP_SERVER] Creating new handler with config: {:?}",
            config
        );
        let port = config.get("port").unwrap().as_u64().unwrap() as u16;
        println!("[HTTP_SERVER] Configured for port {}", port);
        Self { port }
    }

    fn start(
        &self,
        mailbox_tx: mpsc::Sender<ActorMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        println!("Starting http server on port {}", self.port);
        println!(
            "[HTTP_SERVER] Attempting to bind to 127.0.0.1:{}",
            self.port
        );
        Box::pin(async move {
            let state = HttpServerHost::new(self.port, mailbox_tx);
            let mut app = Server::with_state(state);
            app.at("/*").all(HttpServerHost::handle_request);
            println!("[HTTP_SERVER] Setting up routes on /*");

            // Create a channel to signal when we're bound
            let (tx, rx) = tokio::sync::oneshot::channel();

            // Spawn the server in a separate task
            let server_port = self.port;
            tokio::spawn(async move {
                match app.listen(format!("127.0.0.1:{}", server_port)).await {
                    Ok(_) => {
                        println!("[HTTP_SERVER] Successfully bound to port {}", server_port);
                        let _ = tx.send(Ok(()));
                    }
                    Err(e) => {
                        println!(
                            "[HTTP_SERVER] Failed to bind to port {}: {}",
                            server_port, e
                        );
                        let _ = tx.send(Err(anyhow!("Failed to bind HTTP server: {}", e)));
                    }
                }
            });

            // Wait for server to bind
            rx.await
                .map_err(|e| anyhow!("Server startup failed: {}", e))?
                .map_err(|e| e)?;

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
