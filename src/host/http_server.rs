use crate::actor_handle::ActorHandle;
use crate::wasm::WasmActor;
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderName, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Clone)]
pub struct HttpServerHost {
    port: u16,
    actor_handle: ActorHandle,
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

impl HttpServerHost {
    pub fn new(port: u16, actor_handle: ActorHandle) -> Self {
        Self { port, actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports to http-server");
        let _ = self
            .actor_handle
            .with_actor_mut(|actor: &mut WasmActor| -> Result<()> {
                let handle_request_export =
                    actor.find_export("ntwk:theater/http-server", "handle-request")?;
                actor
                    .exports
                    .insert("handle-request".to_string(), handle_request_export);
                info!("Added handle-request export to http-server");
                info!("exports: {:?}", actor.exports);
                Ok(())
            })
            .await;
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let app = Router::new()
            .route("/*path", any(Self::handle_request))
            .with_state(Arc::new(self.actor_handle.clone()));

        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        info!("HTTP server starting on port {}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        axum::serve(listener, app.into_make_service()).await?;

        Ok(())
    }

    async fn handle_request(
        State(actor_handle): State<Arc<ActorHandle>>,
        request: Bytes,
    ) -> Response {
        let http_request = match serde_json::from_slice::<HttpRequest>(&request) {
            Ok(http_request) => http_request,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(format!("Error: {}", e).into())
                    .unwrap_or_default()
            }
        };
        // Changed return type to concrete Response
        info!(
            "Received {} request to {}",
            http_request.method, http_request.uri
        );

        // Convert headers to Vec<(String, String)>
        let headers: Vec<(String, String)> = http_request
            .headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();

        // Get the body bytes
        let body = http_request.body.unwrap_or_default();
        let body_bytes = body.to_vec();

        let http_request = HttpRequest {
            method: http_request.method.to_string(),
            uri: http_request.uri.to_string(),
            headers,
            body: Some(body_bytes),
        };

        let mut actor = actor_handle.inner().lock().await;

        match actor
            .call_func::<(HttpRequest, Vec<u8>), ((HttpResponse, Vec<u8>),)>(
                "handle-request",
                (http_request, actor.actor_state.clone()),
            )
            .await
        {
            Ok(((http_response, new_state),)) => {
                actor.actor_state = new_state;

                // Convert HttpResponse to axum Response
                let mut response = Response::builder()
                    .status(StatusCode::from_u16(http_response.status).unwrap_or(StatusCode::OK));

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
                let response = if let Some(body) = http_response.body {
                    response.body(body.into()).unwrap_or_default()
                } else {
                    response.body(Vec::new().into()).unwrap_or_default()
                };

                response
            }
            Err(e) => Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(format!("Error: {}", e).into())
                .unwrap_or_default(),
        }
    }
}
