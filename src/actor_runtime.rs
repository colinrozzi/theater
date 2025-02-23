use crate::actor_executor::{ActorExecutor, ActorOperation};
use crate::config::{HandlerConfig, ManifestConfig};
use crate::host::filesystem::FileSystemHost;
use crate::host::handler::Handler;
use crate::host::http_client::HttpClientHost;
use crate::host::http_server::HttpServerHost;
use crate::host::message_server::MessageServerHost;
use crate::host::runtime::RuntimeHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::websocket_server::WebSocketServerHost;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::store::ActorStore;
use crate::wasm::{ActorComponent, WasmActor};
use crate::Result;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::time::timeout;
use tracing::{error, info, warn};

const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct RuntimeData {
    pub wrapped_actor: WrappedActor,
    pub operation_rx: mpsc::Receiver<ActorOperation>,
}

pub struct RuntimeComponents {
    pub id: TheaterId,
    pub name: String,
    pub actor_handle: ActorHandle,
    handlers: Vec<Handler>,
    pub runtime_data: Option<RuntimeData>,
}

pub struct ActorRuntime {
    pub actor_id: TheaterId,
    handler_tasks: Vec<tokio::task::JoinHandle<()>>,
    executor_task: tokio::task::JoinHandle<()>,
    actor_handle: ActorHandle,
}

#[derive(Clone)]
pub struct WrappedActor {
    pub actor: Arc<Mutex<WasmActor>>,
}

impl WrappedActor {
    pub fn new(actor: WasmActor) -> Self {
        Self {
            actor: Arc::new(Mutex::new(actor)),
        }
    }

    pub fn inner(&self) -> &Arc<Mutex<WasmActor>> {
        &self.actor
    }

    pub fn take_actor(self) -> Option<WasmActor> {
        Arc::try_unwrap(self.actor)
            .ok()
            .and_then(|mutex| mutex.into_inner().ok())
    }
}

impl ActorRuntime {
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        theater_tx: Sender<TheaterCommand>,
        actor_mailbox: Receiver<ActorMessage>,
    ) -> Result<Self> {
        let mut handlers = Vec::new();

        handlers.push(Handler::MessageServer(MessageServerHost::new(
            actor_mailbox,
            theater_tx.clone(),
        )));

        for handler_config in &config.handlers {
            let handler = match handler_config {
                HandlerConfig::MessageServer(_) => {
                    panic!("MessageServer handler is already added")
                }
                HandlerConfig::HttpServer(config) => {
                    Handler::HttpServer(HttpServerHost::new(config.clone()))
                }
                HandlerConfig::FileSystem(config) => {
                    Handler::FileSystem(FileSystemHost::new(config.clone()))
                }
                HandlerConfig::HttpClient(config) => {
                    Handler::HttpClient(HttpClientHost::new(config.clone()))
                }
                HandlerConfig::Runtime(config) => {
                    Handler::Runtime(RuntimeHost::new(config.clone()))
                }
                HandlerConfig::WebSocketServer(config) => {
                    Handler::WebSocketServer(WebSocketServerHost::new(config.clone()))
                }
                HandlerConfig::Supervisor(config) => {
                    Handler::Supervisor(SupervisorHost::new(config.clone()))
                }
            };
            handlers.push(handler);
        }

        let store = ActorStore::new(id.clone(), theater_tx.clone());
        let actor_component = ActorComponent::new(config, store.clone()).await.expect(
            format!(
                "Failed to create actor component for actor: {:?}",
                config.name
            )
            .as_str(),
        );

        // Add the host functions to the linker of the actor
        {
            for handler in &handlers {
                info!(
                    "Setting up host functions for handler: {:?}",
                    handler.name()
                );
                handler.setup_host_functions(actor_component).await.expect(
                    format!(
                        "Failed to setup host functions for handler: {:?}",
                        handler.name()
                    )
                    .as_str(),
                );
                handler.add_exports(actor_component).await.expect(
                    format!("Failed to add exports for handler: {:?}", handler.name()).as_str(),
                );
            }
        }

        let actor_instance = actor_component
            .instantiate()
            .await
            .expect("Failed to instantiate actor");

        {
            for handler in &handlers {
                info!("Creating functions for handler: {:?}", handler.name());
                handler.add_functions(actor_instance).await.expect(
                    format!(
                        "Failed to create functions for handler: {:?}",
                        handler.name()
                    )
                    .as_str(),
                );
            }
        }

        let actor = WasmActor::new(config, store, &theater_tx).await?;
        let wrapped_actor = WrappedActor::new(actor);

        // Create channels for the executor
        let (operation_tx, operation_rx) = mpsc::channel(32);

        // Create actor handle
        let actor_handle = ActorHandle::new(operation_tx);

        let runtime_data = Some(RuntimeData {
            wrapped_actor,
            operation_rx,
        });

        let components = RuntimeComponents {
            id,
            name: config.name.clone(),
            actor_handle,
            handlers,
            runtime_data,
        };

        // Clone handle for the runtime
        let actor_handle = components.actor_handle.clone();

        let actor = runtime_data
            .wrapped_actor
            .take_actor()
            .expect("Failed to take actor from wrapped actor");

        // Create and spawn executor
        let mut executor = ActorExecutor::new(actor, runtime_data.operation_rx);

        let executor_task = tokio::spawn(async move {
            executor.run().await;
        });

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

        info!("Actor runtime started");

        Ok(Self {
            actor_id: components.id,
            handler_tasks,
            executor_task,
            actor_handle,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        info!("Initiating actor runtime shutdown");

        // First, try to shutdown the actor gracefully
        match timeout(SHUTDOWN_TIMEOUT, self.actor_handle.shutdown()).await {
            Ok(Ok(_)) => info!("Actor shutdown completed successfully"),
            Ok(Err(e)) => warn!("Actor shutdown completed with error: {}", e),
            Err(_) => warn!("Actor shutdown timed out"),
        }

        // Stop all handlers
        for task in self.handler_tasks.drain(..) {
            task.abort();
        }

        // Finally abort the executor if it's still running
        self.executor_task.abort();

        info!("Actor runtime shutdown complete");
        Ok(())
    }
}
