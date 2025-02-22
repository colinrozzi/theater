use crate::actor_handle::ActorHandle;
use crate::actor_executor::ActorError;
use crate::config::HttpClientHandlerConfig;
use crate::wasm::Event;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, error};

#[derive(Clone)]
pub struct HttpClientHost {
    actor_handle: ActorHandle,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    request_id: String,  // Added for tracking requests
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    request_id: String,  // Added to match responses with requests
}

#[derive(Error, Debug)]
pub enum HttpClientError {
    #[error("Request error: {0}")]
    RequestError(String),
    
    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    
    #[error("Invalid method: {0}")]
    InvalidMethod(String),
}

impl HttpClientHost {
    pub fn new(_config: HttpClientHandlerConfig, actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for http-client");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        Ok(())
    }

    async fn handle_request(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let client = reqwest::Client::new();
        
        // Parse HTTP method
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|_| HttpClientError::InvalidMethod(request.method.clone()))?;
        
        // Build request
        let mut http_request = client.request(method, request.uri.clone());
        
        // Add headers
        for (key, value) in request.headers.iter() {
            http_request = http_request.header(key, value);
        }
        
        // Add body if present
        if let Some(body) = request.body.as_ref() {
            http_request = http_request.body(body.clone());
        }
        
        info!("Sending {} request to {}", request.method, request.uri);

        // Send request
        let response = http_request.send().await?;
        
        // Build response
        let response = HttpResponse {
            status: response.status().as_u16(),
            headers: response
                .headers()
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().to_string(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect(),
            body: response.bytes().await.ok().map(|b| b.to_vec()),
            request_id: request.request_id,
        };

        Ok(response)
    }

    pub async fn process_http_event(&self, request: HttpRequest) -> Result<(), HttpClientError> {
        // Handle the request
        let response = match self.handle_request(request).await {
            Ok(response) => response,
            Err(e) => {
                error!("HTTP request failed: {}", e);
                return Err(e);
            }
        };
        
        // Create event with response
        let event = Event {
            event_type: "http-response".to_string(),
            parent: None,
            data: serde_json::to_vec(&response)?,
        };

        // Send event to actor
        self.actor_handle.handle_event(event).await?;

        Ok(())
    }
}
