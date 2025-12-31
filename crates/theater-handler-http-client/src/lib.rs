//! # HTTP Client Handler
//!
//! Provides HTTP client capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to make HTTP requests while maintaining security
//! boundaries and permission controls.
//!
//! ## Features
//!
//! - **Full HTTP method support**: GET, POST, PUT, DELETE, PATCH, etc.
//! - **Headers and body**: Complete control over request headers and body
//! - **Permission-based access**: Control which hosts and methods actors can use
//! - **Event logging**: All HTTP requests are logged to the chain
//!
//! ## Example
//!
//! ```rust,no_run
//! use theater_handler_http_client::HttpClientHandler;
//! use theater::config::actor_manifest::HttpClientHandlerConfig;
//!
//! let config = HttpClientHandlerConfig {};
//! let handler = HttpClientHandler::new(config, None);
//! ```

pub mod events;

pub use events::HttpEventData;

use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tracing::{error, info};
use wasmtime::component::{ComponentType, Lift, Lower};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::actor_manifest::HttpClientHandlerConfig;
use theater::config::enforcement::PermissionChecker;
use theater::config::permissions::HttpClientPermissions;
use theater::events::EventPayload;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

/// HTTP request structure for component model
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

/// HTTP response structure for component model
#[derive(Debug, Clone, Deserialize, Serialize, ComponentType, Lift, Lower)]
#[component(record)]
pub struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

/// Error types for HTTP client operations
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

/// Handler for providing HTTP client capabilities to WebAssembly actors
#[derive(Clone)]
pub struct HttpClientHandler {
    permissions: Option<HttpClientPermissions>,
}

impl HttpClientHandler {
    /// Create a new HTTP client handler
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the HTTP client handler
    /// * `permissions` - Optional permissions controlling HTTP access
    pub fn new(
        _config: HttpClientHandlerConfig,
        permissions: Option<HttpClientPermissions>,
    ) -> Self {
        Self { permissions }
    }

    /// Get the handler name
    pub fn name(&self) -> &str {
        "http-client"
    }

    /// Get the imports
    pub fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/http-client".to_string()])
    }

    /// Get the exports
    pub fn exports(&self) -> Option<Vec<String>> {
        None
    }
}

impl<E> Handler<E> for HttpClientHandler
where
    E: EventPayload + Clone + From<HttpEventData>,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async { Ok(()) })
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent<E>, _ctx: &mut HandlerContext) -> Result<()> {
        // Record setup start
        actor_component.actor_store.record_handler_event(
            "http-client-setup".to_string(),
            HttpEventData::HandlerSetupStart,
            Some("Starting HTTP client host function setup".to_string()),
        );

        info!("Setting up http client host functions");

        let mut interface = match actor_component
            .linker
            .instance("theater:simple/http-client")
        {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_handler_event(
                    "http-client-setup".to_string(),
                    HttpEventData::LinkerInstanceSuccess,
                    Some("Successfully created linker instance".to_string()),
                );
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_handler_event(
                    "http-client-setup".to_string(),
                    HttpEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    },
                    Some(format!("Failed to create linker instance: {}", e)),
                );
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/http-client: {}",
                    e
                ));
            }
        };

        let permissions = self.permissions.clone();
        interface.func_wrap_async(
            "send-http",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore<E>>,
                  (req,): (HttpRequest,)|
                  -> Box<dyn Future<Output = Result<(Result<HttpResponse, String>,)>> + Send> {
                
                // Record HTTP client request call event
                ctx.data_mut().record_handler_event(
                    "theater:simple/http-client/send-http".to_string(),
                    HttpEventData::HttpClientRequestCall {
                        method: req.method.clone(),
                        url: req.uri.clone(),
                        headers_count: req.headers.len(),
                        body: req.body.clone().map(|b| String::from_utf8_lossy(&b).to_string()),
                    },
                    Some(format!("Sending {} request to {}", req.method, req.uri)),
                );
                
                // PERMISSION CHECK BEFORE OPERATION
                // Extract host from URL for permission checking
                let host = if let Ok(parsed_url) = reqwest::Url::parse(&req.uri) {
                    parsed_url.host_str().unwrap_or(&req.uri).to_string()
                } else {
                    req.uri.clone()
                };
                
                if let Err(e) = PermissionChecker::check_http_operation(
                    &permissions,
                    &req.method,
                    &host,
                ) {
                    error!("HTTP client permission denied: {}", e);
                    ctx.data_mut().record_handler_event(
                        "theater:simple/http-client/permission-denied".to_string(),
                        HttpEventData::PermissionDenied {
                            operation: "send-http".to_string(),
                            method: req.method.clone(),
                            url: req.uri.clone(),
                            reason: e.to_string(),
                        },
                        Some(format!("Permission denied for {} request to {}", req.method, req.uri)),
                    );
                    return Box::new(async move {
                        Ok((Err(format!("Permission denied: {}", e)),))
                    });
                }
                
                let req_clone = req.clone();
                
                Box::new(async move {
                    let client = reqwest::Client::new();
                    
                    // Parse method or return error
                    let method = match Method::from_bytes(req_clone.method.as_bytes()) {
                        Ok(m) => m,
                        Err(e) => {
                            let err_msg = format!("Invalid HTTP method: {}", e);

                            // Record error event
                            ctx.data_mut().record_handler_event(
                                "theater:simple/http-client/send-http".to_string(),
                                HttpEventData::Error {
                                    operation: "send-http".to_string(),
                                    path: req_clone.uri.clone(),
                                    message: err_msg.clone(),
                                },
                                Some(format!("Error sending request to {}: {}", req_clone.uri, err_msg)),
                            );
                            
                            return Ok((Err(err_msg),));
                        }
                    };
                    
                    let mut request = client.request(method, req_clone.uri.clone());
                    
                    for (key, value) in req_clone.headers {
                        request = request.header(key, value);
                    }
                    if let Some(body) = req_clone.body {
                        request = request.body(body);
                    }
                    
                    info!("Sending {} request to {}", req_clone.method, req_clone.uri);

                    match request.send().await {
                        Ok(response) => {
                            let status = response.status().as_u16();
                            let headers = response
                                .headers()
                                .iter()
                                .map(|(key, value)| {
                                    (
                                        key.as_str().to_string(),
                                        value.to_str().unwrap_or_default().to_string(),
                                    )
                                })
                                .collect();
                            
                            let body = match response.bytes().await {
                                Ok(bytes) => Some(bytes.to_vec()),
                                Err(e) => {
                                    // Record error reading response body
                                    ctx.data_mut().record_handler_event(
                                        "theater:simple/http-client/send-http".to_string(),
                                        HttpEventData::Error {
                                            operation: "read-response-body".to_string(),
                                            path: req_clone.uri.clone(),
                                            message: e.to_string(),
                                        },
                                        Some(format!("Error reading response body from {}: {}", req_clone.uri, e)),
                                    );
                                    
                                    None
                                }
                            };
                            
                            let resp = HttpResponse {
                                status,
                                headers,
                                body: body.clone(),
                            };
                            
                            // Record HTTP client request result event
                            ctx.data_mut().record_handler_event(
                                "theater:simple/http-client/send-http".to_string(),
                                HttpEventData::HttpClientRequestResult {
                                    status,
                                    headers_count: resp.headers.len(),
                                    body: body.clone().map(|b| String::from_utf8_lossy(&b).to_string()),
                                    success: true,
                                },
                                Some(format!("Received response from {} with status {}", req_clone.uri, status)),
                            );
                            
                            Ok((Ok(resp),))
                        }
                        Err(e) => {
                            let err_msg = e.to_string();

                            // Record HTTP client request error event
                            ctx.data_mut().record_handler_event(
                                "theater:simple/http-client/send-http".to_string(),
                                HttpEventData::Error {
                                    operation: "send-http".to_string(),
                                    path: req_clone.uri.clone(),
                                    message: err_msg.clone(),
                                },
                                Some(format!("Error sending request to {}: {}", req_clone.uri, err_msg)),
                            );
                            
                            Ok((Err(err_msg),))
                        }
                    }
                })
            },
        )?;

        // Record overall setup completion
        actor_component.actor_store.record_handler_event(
            "http-client-setup".to_string(),
            HttpEventData::HandlerSetupSuccess,
            Some("HTTP client host functions setup completed successfully".to_string()),
        );

        info!("Host functions set up for http-client");

        Ok(())
    }

    fn add_export_functions(&self, _actor_instance: &mut ActorInstance<E>) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "http-client"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/http-client".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_creation() {
        let config = HttpClientHandlerConfig {};
        let handler = HttpClientHandler::new(config, None);
        assert_eq!(handler.name(), "http-client");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/http-client".to_string()])
        );
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_handler_clone() {
        let config = HttpClientHandlerConfig {};
        let handler = HttpClientHandler::new(config, None);
        let cloned = handler.clone();
        assert_eq!(cloned.name(), "http-client");
    }

    #[test]
    fn test_http_request_structures() {
        let req = HttpRequest {
            method: "GET".to_string(),
            uri: "https://example.com".to_string(),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: None,
        };
        assert_eq!(req.method, "GET");
        assert_eq!(req.uri, "https://example.com");
    }
}
