use crate::actor_handle::ActorHandle;
use anyhow::Result;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::future::Future;
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
    pub fn new(_config: (), actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        let mut actor = self.actor_handle.inner().lock().await;
        let mut interface = actor
            .linker
            .instance("ntwk:theater/http-client")
            .expect("could not instantiate ntwk:theater/http-client");

        interface.func_wrap_async(
            "http-send",
            |_ctx: wasmtime::StoreContextMut<'_, crate::Store>,
             (req,): (HttpRequest,)|
             -> Box<dyn Future<Output = Result<(HttpResponse,)>> + Send> {
                let client = reqwest::Client::new();
                let mut request =
                    client.request(Method::from_bytes(req.method.as_bytes()).unwrap(), req.uri);
                for (key, value) in req.headers {
                    request = request.header(key, value);
                }
                if let Some(body) = req.body {
                    request = request.body(body);
                }
                Box::new(async move {
                    let response = request.send().await?;
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
                    let body = response.bytes().await?.to_vec();
                    Ok((HttpResponse {
                        status,
                        headers,
                        body: Some(body),
                    },))
                })
            },
        )?;
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        Ok(())
    }
}
