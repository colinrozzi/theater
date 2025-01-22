use crate::actor_handle::ActorHandle;
use crate::config::{HandlerConfig, ManifestConfig};
use crate::host::filesystem::FileSystemHost;
use crate::host::handler::Handler;
use crate::host::http_client::HttpClientHost;
use crate::host::http_server::HttpServerHost;
use crate::host::message_server::MessageServerHost;
use crate::host::runtime::RuntimeHost;
use crate::host::websocket_server::WebSocketServerHost;
use crate::messages::TheaterCommand;
use crate::store::ActorStore;
use crate::wasm::WasmActor;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

pub struct RuntimeComponents {
    pub name: String,
    pub actor_handle: ActorHandle,
    handlers: Vec<Handler>,
}

pub struct ActorRuntime {
    pub actor_id: String,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
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
        let store = ActorStore::new(config.name.clone(), theater_tx.clone());
        let actor = WasmActor::new(config, store).await?;
        let actor_handle = ActorHandle::new(actor);

        let handlers = config
            .handlers
            .iter()
            .map(|handler_config| match handler_config {
                HandlerConfig::MessageServer(config) => Handler::MessageServer(
                    MessageServerHost::new(config.clone(), actor_handle.clone()),
                ),
                HandlerConfig::HttpServer(config) => {
                    Handler::HttpServer(HttpServerHost::new(config.clone(), actor_handle.clone()))
                }
                HandlerConfig::FileSystem(config) => {
                    Handler::FileSystem(FileSystemHost::new(config.clone(), actor_handle.clone()))
                }
                HandlerConfig::HttpClient(config) => {
                    Handler::HttpClient(HttpClientHost::new(config.clone(), actor_handle.clone()))
                }
                HandlerConfig::Runtime(config) => {
                    Handler::Runtime(RuntimeHost::new(config.clone(), actor_handle.clone()))
                }
                HandlerConfig::WebSocketServer(config) => Handler::WebSocketServer(
                    WebSocketServerHost::new(config.clone(), actor_handle.clone()),
                ),
            })
            .collect();

        Ok(RuntimeComponents {
            name: config.name.clone(),
            actor_handle,
            handlers,
        })
    }

    pub async fn start(components: RuntimeComponents) -> Result<Self> {
        {
            for handler in &components.handlers {
                let _ = handler.setup_host_function().await;
                let _ = handler.add_exports().await;
            }
        }

        let mut handler_tasks = Vec::new();
        // Start all handlers
        for mut handler in components.handlers {
            let task = tokio::spawn(async move {
                if let Err(e) = handler.start().await {
                    error!("Handler failed: {}", e);
                }
            });
            handler_tasks.push(task);
        }

        {
            let mut actor = components.actor_handle.inner().lock().await;
            actor.init().await;
        }

        info!("Actor runtime started");

        Ok(Self {
            actor_id: components.name,
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
