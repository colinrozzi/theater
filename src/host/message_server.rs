use crate::actor_handle::ActorHandle;
use crate::wasm::ActorState;
use crate::wasm::Event;
use anyhow::Result;
use thiserror::Error;
use tide::{Body, Request, Response, Server};
use tracing::info;

#[derive(Clone)]
pub struct MessageServerHost {
    port: u16,
    actor_handle: ActorHandle,
}

#[derive(Error, Debug)]
pub enum MessageServerError {
    #[error("Calling WASM error: {context} - {message}")]
    WasmError {
        context: &'static str,
        message: String,
    },
}

impl MessageServerHost {
    pub fn new(port: u16, actor_handle: ActorHandle) -> Self {
        Self { port, actor_handle }
    }

    pub fn setup_host_functions(&self) -> Result<()> {
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
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
        let evt: Event = serde_json::from_slice(&body_bytes)?;

        info!("Received event: {:?}", evt);
        let mut actor = req.state().inner().lock().await;
        let (new_state,) = actor
            .call_func::<(Event, ActorState), (ActorState,)>(
                "handle-request",
                (evt, actor.actor_state.clone()),
            )
            .await
            .expect("Failed to call handle-request");

        actor.actor_state = new_state;

        info!("success");

        Ok(Response::builder(200)
            .body(Body::from_string("Request forwarded to actor".to_string()))
            .build())
    }
}
