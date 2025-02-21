use crate::actor_handle::ActorHandle;
use crate::config::HttpClientHandlerConfig;
use crate::ActorStore;
use crate::host::host_wrapper::HostFunctionBoundary;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::future::Future;
use tracing::info;
use wasmtime::component::{ComponentType, Lift, Lower};

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

impl HttpClientHost {
    pub fn new(_config: HttpClientHandlerConfig, actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for http-client");
        let mut actor = self.actor_handle.inner().lock().await;
        let mut interface = actor
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
                    let request = boundary.wrap(&mut ctx, (req_clone.clone(),), |req| {
                        let client = reqwest::Client::new();
                        let mut request = client.request(
                            Method::from_bytes(req.method.as_bytes()).unwrap(),
                            req.uri.clone(),
                        );
                        for (key, value) in req.headers {
                            request = request.header(key, value);
                        }
                        if let Some(body) = req.body {
                            request = request.body(body);
                        }
                        info!("Sending {} request to {}", req.method, req.uri);
                        
                        Ok(request)
                    })?;

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
                            
                            // Need to await the body here
                            let body = response.bytes().await.ok().map(|b| b.to_vec());
                            
                            let resp = HttpResponse {
                                status,
                                headers,
                                body,
                            };
                            
                            // Record the response
                            boundary.wrap(&mut ctx, resp.clone(), |resp| Ok((Ok(resp),)))
                        }
                        Err(e) => {
                            let err = e.to_string();
                            boundary.wrap(&mut ctx, err.clone(), |err| Ok((Err(err),)))
                        }
                    }
                })
            },
        )?;
        
        info!("Host functions set up for http-client");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        Ok(())
    }
}
