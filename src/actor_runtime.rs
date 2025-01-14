use crate::actor::ActorCommandErased;
use crate::actor::ActorHandle;
use crate::actor::WasmActor;
use crate::config::{HandlerConfig, ManifestConfig};
use crate::host::handler::Handler;
use crate::host::http_server::HttpServerHost;
use crate::host::message_server::MessageServerHost;
use crate::messages::TheaterCommand;
use crate::store::Store;
use crate::Result;
use std::path::PathBuf;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tracing::{error, info};

pub struct RuntimeComponents {
    pub name: String,
    pub actor_rx: Receiver<ActorCommandErased>,
    pub actor: WasmActor,
    handlers: Vec<Handler>,
}

pub struct ActorRuntime {
    pub actor_id: String,
    handler_tasks: Vec<JoinHandle<()>>,
    actor_task: JoinHandle<()>,
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
        let (actor_tx, actor_rx) = tokio::sync::mpsc::channel(100);
        let actor = WasmActor::new(config, store).await?;
        let actor_handle = ActorHandle::new(actor_tx);

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
            actor_rx,
            actor,
            handlers,
        })
    }

    pub async fn start(components: RuntimeComponents) -> Result<Self> {
        let mut handler_tasks = Vec::new();

        // Start all handlers
        for handler in components.handlers {
            let task = tokio::spawn(async move {
                let _ = handler.setup_host_function().await;
                let _ = handler.add_exports().await;

                if let Err(e) = handler.start().await {
                    error!("Handler failed: {}", e);
                }
            });
            handler_tasks.push(task);
        }

        let actor_task = tokio::spawn(async move {
            let _ = components.actor.run(components.actor_rx).await;
        });

        info!("Actor runtime started");

        Ok(Self {
            actor_id: components.name,
            actor_task,
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
