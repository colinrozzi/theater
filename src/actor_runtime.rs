use crate::actor_handle::ActorHandle;
use crate::actor_process::{ActorProcess, ProcessMessage};
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
    actor_process: ActorProcess,
    process_tx: mpsc::Sender<ProcessMessage>,
}

pub struct ActorRuntime {
    pub actor_id: String,
    process_handle: Option<tokio::task::JoinHandle<()>>,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
}

pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
}

impl Handler {
    pub async fn start(&self, process_tx: mpsc::Sender<ProcessMessage>) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => handler.start(process_tx).await,
            Handler::HttpServer(handler) => handler.start(process_tx).await,
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
        let (process_tx, process_rx) = mpsc::channel(32);

        let store = Store::new(config.name.clone(), theater_tx.clone());
        let actor = WasmActor::new(config, store).await?;
        let actor_handler = ActorHandle::new(actor);

        let handlers = config
            .handlers
            .iter()
            .map(|handler_config| match handler_config {
                HandlerConfig::MessageServer(config) => {
                    Handler::MessageServer(MessageServerHost::new(config.port))
                }
                HandlerConfig::HttpServer(config) => {
                    Handler::HttpServer(HttpServerHost::new(config.port, actor_handler.clone()))
                }
            })
            .collect();

        // Create and spawn actor process
        let actor_process = ActorProcess::new(&config.name, actor_handler, process_rx).await?;

        Ok(RuntimeComponents {
            name: config.name.clone(),
            handlers,
            actor_process,
            process_tx,
        })
    }

    pub async fn start(mut components: RuntimeComponents) -> Result<Self> {
        let mut handler_tasks = Vec::new();

        // Start all handlers
        for handler in components.handlers {
            let tx = components.process_tx.clone();
            let task = tokio::spawn(async move {
                if let Err(e) = handler.start(tx).await {
                    error!("Handler failed: {}", e);
                }
            });
            handler_tasks.push(task);
        }

        // Start the actor process
        let process_handle = Some(tokio::spawn(async move {
            if let Err(e) = components.actor_process.run().await {
                error!("Actor process failed: {}", e);
            }
        }));

        info!("Actor runtime started");

        Ok(Self {
            actor_id: components.name,
            process_handle,
            handler_tasks,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        // Stop all handlers
        for task in self.handler_tasks.drain(..) {
            task.abort();
        }

        // Cancel actor process
        if let Some(handle) = self.process_handle.take() {
            handle.abort();
        }

        Ok(())
    }
}
