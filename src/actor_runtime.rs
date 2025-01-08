use crate::actor_handle::ActorHandle;
use crate::config::{HandlerConfig, ManifestConfig};
use crate::http_server::HttpServerHost;
use crate::message_server::MessageServerHost;
use crate::messages::TheaterCommand;
use crate::store::Store;
use crate::wasm::WasmActor;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};
use wasmtime::component::Linker;

pub struct RuntimeComponents {
    pub name: String,
    handlers: Vec<Handler>,
    actor_handle: ActorHandle,
}

pub struct ActorRuntime {
    pub actor_id: String,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
    actor_handle: ActorHandle,
}

pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
}

impl Handler {
    pub fn new(handler_config: HandlerConfig, actor_handle: ActorHandle) -> Self {
        match handler_config {
            HandlerConfig::MessageServer(config) => {
                Handler::MessageServer(MessageServerHost::new(config.port, actor_handle))
            }
            HandlerConfig::HttpServer(config) => {
                Handler::HttpServer(HttpServerHost::new(config.port, actor_handle))
            }
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.start().await,
            Handler::HttpServer(handler) => handler.start().await,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Handler::MessageServer(_) => "message-server".to_string(),
            Handler::HttpServer(_) => "http-server".to_string(),
        }
    }

    pub async fn setup_host_function(&self, linker: &mut Linker<Store>) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.setup_host_function(linker)?,
            Handler::HttpServer(handler) => handler.setup_host_function(linker)?,
        }
        Ok(())
    }
}

impl ActorRuntime {
    pub async fn from_file(
        manifest_path: PathBuf,
        theater_tx: Sender<TheaterCommand>,
    ) -> Result<RuntimeComponents> {
        // Load manifest config
        let config = ManifestConfig::from_file(&manifest_path)?;
        let runtime = Self::new(&config, theater_tx).await?;
        Ok(runtime)
    }

    pub async fn new(
        config: &ManifestConfig,
        theater_tx: Sender<TheaterCommand>,
    ) -> Result<RuntimeComponents> {
        Self::init_components(config, theater_tx).await
    }

    async fn init_components(
        config: &ManifestConfig,
        theater_tx: Sender<TheaterCommand>,
    ) -> Result<RuntimeComponents> {
        let store = Store::new(config.name.clone(), theater_tx.clone());
        let actor = WasmActor::new(config, store).await?;
        actor.call_func("init", ()).await?;
        let actor_handle = ActorHandle::new(actor);

        let handlers = config
            .handlers
            .iter()
            .map(|handler_config| match handler_config {
                HandlerConfig::MessageServer(config) => Handler::MessageServer(
                    MessageServerHost::new(config.port, actor_handle.clone()),
                ),
                HandlerConfig::HttpServer(config) => {
                    Handler::HttpServer(HttpServerHost::new(config.port, actor_handle.clone()))
                }
            })
            .collect();

        Ok(RuntimeComponents {
            name: config.name.clone(),
            handlers,
            actor_handle,
        })
    }

    pub async fn start(components: RuntimeComponents) -> Result<Self> {
        let mut handler_tasks = Vec::new();

        // Start all handlers
        for handler in components.handlers {
            let task = tokio::spawn(async move {
                if let Err(e) = handler.start().await {
                    error!("Handler failed: {}", e);
                }
            });
            handler_tasks.push(task);
        }

        info!("Actor runtime started");

        Ok(Self {
            actor_id: components.name,
            actor_handle: components.actor_handle,
            handler_tasks,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        // Stop all handlers
        for task in self.handler_tasks.drain(..) {
            task.abort();
        }

        Ok(())
    }
}
