use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::config::actor_manifest::HttpClientHandlerConfig;
use crate::config::enforcement::PermissionChecker;
use crate::events::http::HttpEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::future::Future;
use thiserror::Error;
use tracing::{error, info};
use wasmtime::component::{ComponentType, Lift, Lower};

#[derive(Clone)]
pub struct HttpClientHost {
    permissions: Option<crate::config::permissions::HttpClientPermissions>,
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
    pub fn new(_config: HttpClientHandlerConfig, permissions: Option<crate::config::permissions::HttpClientPermissions>) -> Self {
        Self { permissions }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up http client host functions");

        let mut interface = actor_component
            .linker
            .instance("theater:simple/http-client")
            .expect("could not instantiate theater:simple/http-client");

        let permissions = self.permissions.clone();
        interface.func_wrap_async(
            "send-http",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (req,): (HttpRequest,)|
                  -> Box<dyn Future<Output = Result<(Result<HttpResponse, String>,)>> + Send> {
                
                // Record HTTP client request call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/http-client/send-http".to_string(),
                    data: EventData::Http(HttpEventData::HttpClientRequestCall {
                        method: req.method.clone(),
                        url: req.uri.clone(),
                        headers_count: req.headers.len(),
                        body: req.body.clone().map(|b| String::from_utf8_lossy(&b).to_string()),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Sending {} request to {}", req.method, req.uri)),
                });
                
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
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/http-client/permission-denied".to_string(),
                        data: EventData::Http(HttpEventData::PermissionDenied {
                            operation: "send-http".to_string(),
                            method: req.method.clone(),
                            url: req.uri.clone(),
                            reason: e.to_string(),
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Permission denied for {} request to {}", req.method, req.uri)),
                    });
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
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/http-client/send-http".to_string(),
                                data: EventData::Http(HttpEventData::Error {
                                    operation: "send-http".to_string(),
                                    path: req_clone.uri.clone(),
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error sending request to {}: {}", req_clone.uri, err_msg)),
                            });
                            
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
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: "theater:simple/http-client/send-http".to_string(),
                                        data: EventData::Http(HttpEventData::Error {
                                            operation: "read-response-body".to_string(),
                                            path: req_clone.uri.clone(),
                                            message: e.to_string(),
                                        }),
                                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                        description: Some(format!("Error reading response body from {}: {}", req_clone.uri, e)),
                                    });
                                    
                                    None
                                }
                            };
                            
                            let resp = HttpResponse {
                                status,
                                headers,
                                body: body.clone(),
                            };
                            
                            // Record HTTP client request result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/http-client/send-http".to_string(),
                                data: EventData::Http(HttpEventData::HttpClientRequestResult {
                                    status,
                                    headers_count: resp.headers.len(),
                                    body: body.clone().map(|b| String::from_utf8_lossy(&b).to_string()),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Received response from {} with status {}", req_clone.uri, status)),
                            });
                            
                            Ok((Ok(resp),))
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            
                            // Record HTTP client request error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/http-client/send-http".to_string(),
                                data: EventData::Http(HttpEventData::Error {
                                    operation: "send-http".to_string(),
                                    path: req_clone.uri.clone(),
                                    message: err_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error sending request to {}: {}", req_clone.uri, err_msg)),
                            });
                            
                            Ok((Err(err_msg),))
                        }
                    }
                })
            },
        )?;

        info!("Host functions set up for http-client");

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        Ok(())
    }

    pub async fn start(
        &self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        Ok(())
    }
}
