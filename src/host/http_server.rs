use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::HttpServerHandlerConfig;
use crate::wasm::{ActorComponent, ActorInstance};
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
use tracing::{error, info};
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Clone)]
pub struct HttpServerHost {
    port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
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
    pub fn new(config: HttpServerHandlerConfig) -> Self {
        Self { port: config.port }
    }

    pub async fn setup_host_functions(&self, _actor_component: &mut ActorComponent) -> Result<()> {
        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        actor_instance.register_function::<(HttpRequest,), (HttpResponse,)>(
            "ntwk:theater/http-server",
            "handle-request",
        )
    }

    pub async fn start(&self, actor_handle: ActorHandle) -> Result<()> {
        let app = Router::new()
            .route("/", any(Self::handle_request))
            .route("/{*wildcard}", any(Self::handle_request))
            .with_state(Arc::new(actor_handle.clone()));
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

        // Call function and serialize/deserialize
        let results = actor_handle
            .call_function::<(HttpRequest,), (HttpResponse,)>(
                "ntwk:theater/http-server.handle-request".to_string(),
                (http_request,)
            )
            .await
            .map(|(response,)| response)  // Extract the response from the tuple
            .expect("Failed to call function");
            
        let http_response = results;

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
}
