use crate::actor_handle::ActorHandle;
use crate::store::Store;
use crate::wasm::Event;
use anyhow::Result;
use serde_json::json;
use tide::{Body, Request, Response, Server};
use tracing::info;
use wasmtime::component::Linker;

#[derive(Clone)]
pub struct MessageServerHost {
    port: u16,
    actor_handle: ActorHandle,
}

impl MessageServerHost {
    pub fn new(port: u16, actor_handle: ActorHandle) -> Self {
        Self { port, actor_handle }
    }

    pub fn setup_host_function(&self, _linker: &mut Linker<Store>) -> Result<()> {
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let mut app = Server::with_state(self.actor_handle.clone());
        app.at("/*").all(Self::handle_request);
        app.at("/").all(Self::handle_request);

        info!("Message server starting on port {}", self.port);
        app.listen(format!("127.0.0.1:{}", self.port)).await?;

        Ok(())
    }

    async fn handle_request(mut req: Request<ActorHandle>) -> tide::Result {
        info!("Received {} request to {}", req.method(), req.url().path());

        // Get the body bytes
        let body_bytes = req.body_bytes().await?.to_vec();

        let evt = Event {
            type_: "actor_message".to_string(),
            data: json!(body_bytes),
        };

        let evt_bytes = serde_json::to_vec(&evt)?;

        req.state()
            .with_actor(|actor| {
                actor.call_func::<(Vec<u8>,), ()>("handle", (evt_bytes,));
                Ok(())
            })
            .await?;

        Ok(Response::builder(200)
            .body(Body::from_string("Request forwarded to actor".to_string()))
            .build())
    }
}
