use crate::actor_executor::ActorExecutor;
use crate::actor_handle::ActorHandle;
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
use crate::wasm::ActorComponent;
use crate::Result;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tracing::{info, warn};

const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct ActorRuntime {
    pub actor_id: TheaterId,
    handler_tasks: Vec<JoinHandle<()>>,
    actor_executor_task: JoinHandle<()>,
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
        let mut actor_component = ActorComponent::new(config, store.clone()).await.expect(
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
                handler
                    .setup_host_functions(&mut actor_component)
                    .await
                    .expect(
                        format!(
                            "Failed to setup host functions for handler: {:?}",
                            handler.name()
                        )
                        .as_str(),
                    );
            }
        }

        let mut actor_instance = actor_component
            .instantiate()
            .await
            .expect("Failed to instantiate actor");

        {
            for handler in &handlers {
                info!("Creating functions for handler: {:?}", handler.name());
                handler
                    .add_export_functions(&mut actor_instance)
                    .await
                    .expect(
                        format!(
                            "Failed to create functions for handler: {:?}",
                            handler.name()
                        )
                        .as_str(),
                    );
            }
        }

        let (operation_tx, operation_rx) = tokio::sync::mpsc::channel(100);

        let actor_handle = ActorHandle::new(operation_tx);

        let mut handler_tasks: Vec<JoinHandle<()>> = Vec::new();

        for mut handler in handlers {
            info!("Starting handler: {:?}", handler.name());
            let actor_handle = actor_handle.clone();
            let handler_task = tokio::spawn(async move {
                if let Err(e) = handler.start(actor_handle).await {
                    warn!("Handler failed: {:?}", e);
                }
            });
            handler_tasks.push(handler_task);
        }

        let init_state = config.load_init_state().expect("Failed to load init state");
        actor_instance.store.data_mut().set_state(init_state);

        let mut actor_executor = ActorExecutor::new(actor_instance, operation_rx);
        let executor_task = tokio::spawn(async move { actor_executor.run().await });

        Ok(ActorRuntime {
            actor_id: id.clone(),
            handler_tasks,
            actor_executor_task: executor_task,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        info!("Initiating actor runtime shutdown");

        // Stop all handlers
        for task in self.handler_tasks.drain(..) {
            task.abort();
        }

        // Finally abort the executor if it's still running
        self.actor_executor_task.abort();

        info!("Actor runtime shutdown complete");
        Ok(())
    }
}
