use crate::actor::Event;
use crate::actor_process::ActorMessage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tide::listener::Listener;
use tide::{Body, Request, Response, Server};
use tokio::sync::mpsc;
use tracing::info;

#[derive(Clone, Debug)]
pub struct HttpServerHost {
    port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

impl HttpServerHost {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub async fn start(&self, mailbox_tx: mpsc::Sender<ActorMessage>) -> Result<()> {
        info!("HTTP-SERVER starting on port {}", self.port);
        let mut app = Server::with_state(mailbox_tx.clone());
        app.at("/*").all(Self::handle_request);
        app.at("/").all(Self::handle_request);

        // First bind the server
        let mut listener = app.bind(format!("127.0.0.1:{}", self.port)).await?;

        info!("HTTP-SERVER starting on port {}", self.port);

        // Then start accepting connections
        listener.accept().await?;

        Ok(())
    }

    async fn handle_request(mut req: Request<mpsc::Sender<ActorMessage>>) -> tide::Result {
        info!("Received {} request to {}", req.method(), req.url().path());

        // Create a channel for receiving the response
        let (response_tx, mut response_rx) = mpsc::channel(1);

        // Get the body bytes
        let body_bytes = req.body_bytes().await?.to_vec();

        let http_request = HttpRequest {
            method: req.method().to_string(),
            uri: req.url().path().to_string(),
            headers: req
                .header_names()
                .map(|name| {
                    (
                        name.to_string(),
                        req.header(name).unwrap().iter().next().unwrap().to_string(),
                    )
                })
                .collect(),
            body: Some(body_bytes),
        };

        let evt = Event {
            type_: "http_request".to_string(),
            data: json!(http_request),
        };

        let msg = ActorMessage {
            event: evt,
            response_channel: Some(response_tx),
        };

        // Send to actor
        req.state()
            .send(msg)
            .await
            .map_err(|_| tide::Error::from_str(500, "Failed to forward request to actor"))?;

        // Wait for response
        let actor_response = response_rx.recv().await.unwrap();

        // actor response will come back as an Event, with the HTTP response in the data field
        let http_response: HttpResponse = serde_json::from_value(actor_response.data).unwrap();

        // print the type of actor response
        // Process actor response
        // will come back as a serde_json::Value, so we need to convert it to a HttpResponse
        //let http_response: HttpResponse = serde_json::from_value(actor_response).unwrap();

        let mut response = Response::new(http_response.status);

        for (key, value) in http_response.headers {
            response.insert_header(key.as_str(), value.as_str());
        }

        if let Some(body) = http_response.body {
            response.set_body(Body::from_bytes(body));
        }

        Ok(response)
    }
}
