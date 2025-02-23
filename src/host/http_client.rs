use crate::actor_handle::ActorHandle;
use wasmtime::component::{Lift, Lower, ComponentType};
use crate::actor_executor::ActorError;
use crate::config::HttpClientHandlerConfig;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::store::ActorStore;
use crate::wasm::{ActorComponent, ActorInstance};
use std::future::Future;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, error};

#[derive(Clone)]
pub struct HttpClientHost {
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
        Self { actor_handle}
    }

    pub async fn setup_host_functions(&self, mut actor_component: ActorComponent) -> Result<()> {
        info!("Setting up http client host functions");

 let mut interface = actor_component
            .linker
            .instance("ntwk:theater/http-client")
            .expect("could not instantiate ntwk:theater/http-client");

        let boundary = HostFunctionBoundary::new("ntwk:theater/http-client", "send-http");

        interface.func_wrap_async(
            "send-http",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (req,): (HttpRequest,)|
                  -> Box<dyn Future<Output = Result<(Result<HttpResponse, String>,)>> + Send> {
                let req_clone = req.clone();
                let boundary = boundary.clone();
                
                Box::new(async move {
                    // Record the outbound request
                    boundary.wrap(&mut ctx, req_clone.clone(), |req| Ok(req))?;

                    let client = reqwest::Client::new();
                    let mut request = client.request(
                        Method::from_bytes(req_clone.method.as_bytes()).unwrap(),
                        req_clone.uri.clone(),
                    );
                    
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
                                        value.to_str().unwrap().to_string(),
                                    )
                                })
                                .collect();
                            
                            let body = response.bytes().await.ok().map(|b| b.to_vec());
                            
                            let resp = HttpResponse {
                                status,
                                headers,
                                body,
                            };
                            
                            // Record the response
                            boundary.wrap(&mut ctx, resp.clone(), |r| Ok((Ok(r),)))
                        }
                        Err(e) => {
                            let err = e.to_string();
                            boundary.wrap(&mut ctx, err.clone(), |e| Ok((Err(e),)))
                        }
                    }
                })
            },
        )?;
        
        info!("Host functions set up for http-client");

        Ok(())
    }

    pub async fn add_exports(&self, _actor_component: ActorComponent) -> Result<()> {
        Ok(())
    }

    pub async fn add_functions(&self, _actor_instance: ActorInstance) -> Result<()> {
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
        };

        Ok(response)
    }
}
