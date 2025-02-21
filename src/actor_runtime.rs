use crate::actor_handle::ActorHandle;
use crate::config::{HandlerConfig, ManifestConfig};
use crate::host::filesystem::FileSystemHost;
use crate::host::handler::Handler;
use crate::host::http_client::HttpClientHost;
use crate::host::http_server::HttpServerHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::message_server::MessageServerHost;
use crate::host::runtime::RuntimeHost;
use crate::host::websocket_server::WebSocketServerHost;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::store::ActorStore;
use crate::wasm::WasmActor;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{error, info};

pub struct RuntimeComponents {
    pub id: TheaterId,
    pub name: String,
    pub actor_handle: ActorHandle,
    handlers: Vec<Handler>,
}

pub struct ActorRuntime {
    pub actor_id: TheaterId,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl ActorRuntime {
    pub async fn from_file(
        manifest_path: PathBuf,
        theater_tx: Sender<TheaterCommand>,
        actor_mailbox: Receiver<ActorMessage>,
    ) -> Result<RuntimeComponents> {
        // Load manifest config
        let config = ManifestConfig::from_file(&manifest_path)?;
        let runtime = Self::new(&config, theater_tx, actor_mailbox).await?;
        Ok(runtime)
    }

    pub async fn new(
        config: &ManifestConfig,
        theater_tx: Sender<TheaterCommand>,
        actor_mailbox: Receiver<ActorMessage>,
    ) -> Result<RuntimeComponents> {
        Self::init_components(config, theater_tx, actor_mailbox).await
    }

    async fn init_components(
        config: &ManifestConfig,
        theater_tx: Sender<TheaterCommand>,
        actor_mailbox: Receiver<ActorMessage>,
    ) -> Result<RuntimeComponents> {
        let id = TheaterId::generate();

        let span = tracing::info_span!("actor", actor_id = %id, actor_name = %config.name);
        let _enter = span.enter();

        let store = ActorStore::new(id.clone(), theater_tx.clone());
        let actor = WasmActor::new(config, store, &theater_tx).await?;
        let actor_handle = ActorHandle::new(actor);

        let mut handlers = Vec::new();

        handlers.push(Handler::MessageServer(MessageServerHost::new(
            actor_mailbox,
            theater_tx.clone(),
            actor_handle.clone(),
        )));

        for handler_config in &config.handlers {
            let handler = match handler_config {
                HandlerConfig::MessageServer(_) => {
                    panic!("MessageServer handler is already added")
                }
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
                HandlerConfig::Supervisor(config) => Handler::Supervisor(
                    SupervisorHost::new(config.clone(), actor_handle.clone()),
                ),
            };
            handlers.push(handler);
        }

        Ok(RuntimeComponents {
            id,
            name: config.name.clone(),
            actor_handle,
            handlers,
        })
    }

    pub async fn start(components: RuntimeComponents) -> Result<Self> {
        {
            for handler in &components.handlers {
                info!(
                    "Setting up host functions for handler: {:?}",
                    handler.name()
                );
                handler.setup_host_function().await.expect(
                    format!(
                        "Failed to setup host functions for handler: {:?}",
                        handler.name()
                    )
                    .as_str(),
                );
                info!("Adding exports for handler: {:?}", handler.name());
                handler.add_exports().await.expect(
                    format!("Failed to add exports for handler: {:?}", handler.name()).as_str(),
                );
            }
        }

        let mut handler_tasks = Vec::new();
        // Start all handlers
        info!("Starting handlers");
        for mut handler in components.handlers {
            let task = tokio::spawn(async move {
                if let Err(e) = handler.start().await {
                    error!("Handler failed: {}", e);
                }
            });
            handler_tasks.push(task);
        }

        info!("Running init on actor");
        {
            let mut actor = components.actor_handle.inner().lock().await;
            actor.init().await;
        }

        info!("Actor runtime started");

        Ok(Self {
            actor_id: components.id,
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
