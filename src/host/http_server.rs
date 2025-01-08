use crate::actor_handle::ActorHandle;
use crate::store::Store;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tide::listener::Listener;
use tide::{Body, Request, Response, Server};
use tracing::info;
use wasmtime::component::{ComponentType, Lift, Linker, Lower};

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

    pub fn setup_host_function(&self, _linker: &mut Linker<Store>) -> Result<()> {
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        info!("HTTP-SERVER starting on port {}", self.port);
        let mut app = Server::with_state(self.actor_handle.clone());
        app.at("/*").all(Self::handle_request);
        app.at("/").all(Self::handle_request);

        // First bind the server
        let mut listener = app.bind(format!("127.0.0.1:{}", self.port)).await?;

        info!("HTTP-SERVER starting on port {}", self.port);

        // Then start accepting connections
        listener.accept().await?;

        Ok(())
    }

    async fn handle_request(mut req: Request<ActorHandle>) -> tide::Result {
        info!("Received {} request to {}", req.method(), req.url().path());

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

        let handle_result = req
            .state()
            .with_actor_owned(|actor| {
                let request = http_request.clone();
                Ok(actor.call_func_async::<(HttpRequest,), (HttpResponse,)>(
                    "handle_http_request",
                    (request,),
                ))
            })
            .await;

        // Get the response out of the handle result
        let http_response = handle_result.unwrap().await.unwrap().0;

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
