use crate::actor_handle::ActorHandle;
use crate::actor_executor::ActorError;
use crate::config::HttpServerHandlerConfig;
use crate::wasm::Event;
use anyhow::Result;
use axum::{
    extract::State,
    http::{HeaderName, HeaderValue, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, error};

#[derive(Clone)]
pub struct HttpServerHost {
    port: u16,
    actor_handle: ActorHandle,
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

#[derive(Error, Debug)]
pub enum HttpServerError {
    #[error("Handler error: {0}")]
    HandlerError(String),
    
    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl HttpServerHost {
    pub fn new(config: HttpServerHandlerConfig, actor_handle: ActorHandle) -> Self {
        Self {
            port: config.port,
            actor_handle,
        }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports to http-server");
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let app = Router::new()
            .route("/", any(Self::handle_request))
            .route("/{*wildcard}", any(Self::handle_request))
            .with_state(Arc::new(self.actor_handle.clone()));
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("Starting http server on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Listening on {}", addr);
        axum::serve(listener, app.into_make_service()).await?;
        info!("Server started");
        Ok(())
    }

    async fn handle_request(
        State(actor_handle): State<Arc<ActorHandle>>,
        req: axum::http::Request<axum::body::Body>,
    ) -> Response {
        info!("Handling request");

        // Convert axum request to HttpRequest
        let (parts, body) = req.into_parts();
        let body_bytes = match axum::body::to_bytes(body, 100 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Failed to read request body: {}", e);
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body("Failed to read request body".into())
                    .unwrap_or_default();
            }
        };

        let headers = parts
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();

        let http_request = HttpRequest {
            method: parts.method.as_str().to_string(),
            uri: parts.uri.to_string(),
            headers,
            body: Some(body_bytes.to_vec()),
        };

        info!(
            "Received {} request to {}",
            http_request.method, http_request.uri
        );

        // Create event for request handling
        let event = match Self::create_request_event(&http_request) {
            Ok(event) => event,
            Err(e) => {
                error!("Failed to create request event: {}", e);
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body("Failed to process request".into())
                    .unwrap_or_default();
            }
        };

        // Handle the event
        if let Err(e) = actor_handle.handle_event(event).await {
            error!("Failed to handle request: {}", e);
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body("Failed to process request".into())
                .unwrap_or_default();
        }

        // Get the updated state which should contain our response
        let state = match actor_handle.get_state().await {
            Ok(state) => state,
            Err(e) => {
                error!("Failed to get state after request: {}", e);
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body("Failed to process request".into())
                    .unwrap_or_default();
            }
        };

        // Deserialize response from state
        let http_response: HttpResponse = match serde_json::from_slice(&state) {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to deserialize response: {}", e);
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body("Failed to process response".into())
                    .unwrap_or_default();
            }
        };

        // Convert HttpResponse to axum Response
        let mut response = Response::builder()
            .status(StatusCode::from_u16(http_response.status).unwrap_or(StatusCode::OK));

        // Add headers
        if let Some(headers) = response.headers_mut() {
            for (key, value) in http_response.headers {
                if let Ok(header_value) = HeaderValue::from_str(&value) {
                    if let Ok(header_name) = key.parse::<HeaderName>() {
                        headers.insert(header_name, header_value);
                    }
                }
            }
        }

        // Add body if present
        if let Some(body) = http_response.body {
            response.body(body.into()).unwrap_or_default()
        } else {
            response.body(Vec::new().into()).unwrap_or_default()
        }
    }

    fn create_request_event(request: &HttpRequest) -> Result<Event, HttpServerError> {
        let data = serde_json::to_vec(request)?;
        Ok(Event {
            event_type: "handle-request".to_string(),
            parent: None,
            data,
        })
    }
}
