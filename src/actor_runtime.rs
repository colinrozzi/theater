use crate::actor_executor::ActorExecutor;
use crate::actor_executor::ActorOperation;
use crate::actor_handle::ActorHandle;
use crate::actor_store::ActorStore;
use crate::config::{HandlerConfig, ManifestConfig};
use crate::host::filesystem::FileSystemHost;
use crate::host::framework::HttpFramework;
use crate::host::handler::Handler;
use crate::host::http_client::HttpClientHost;
use crate::host::message_server::MessageServerHost;
use crate::host::runtime::RuntimeHost;
use crate::host::store::StoreHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::timing::TimingHost;
use crate::id::TheaterId;
use crate::messages::{ActorMessage, TheaterCommand};
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use crate::wasm::ActorComponent;
use crate::Result;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

#[allow(dead_code)]
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct ActorRuntime {
    pub actor_id: TheaterId,
    handler_tasks: Vec<JoinHandle<()>>,
    actor_executor_task: JoinHandle<()>,
    shutdown_controller: ShutdownController,
}

impl ActorRuntime {
    pub async fn start(
        id: TheaterId,
        config: &ManifestConfig,
        state_bytes: Option<Vec<u8>>,
        theater_tx: Sender<TheaterCommand>,
        actor_mailbox: Receiver<ActorMessage>,
        operation_rx: Receiver<ActorOperation>,
        operation_tx: Sender<ActorOperation>,
        init: bool,
        parent_shutdown_receiver: ShutdownReceiver,
    ) -> Result<Self> {
        // Create a local shutdown controller for this runtime
        let (shutdown_controller, _) = ShutdownController::new();
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
                HandlerConfig::FileSystem(config) => {
                    Handler::FileSystem(FileSystemHost::new(config.clone()))
                }
                HandlerConfig::HttpClient(config) => {
                    Handler::HttpClient(HttpClientHost::new(config.clone()))
                }
                HandlerConfig::HttpFramework(_) => Handler::HttpFramework(HttpFramework::new()),
                HandlerConfig::Runtime(config) => {
                    Handler::Runtime(RuntimeHost::new(config.clone()))
                }
                HandlerConfig::Supervisor(config) => {
                    Handler::Supervisor(SupervisorHost::new(config.clone()))
                }
                HandlerConfig::Store(config) => Handler::Store(StoreHost::new(config.clone())),
                HandlerConfig::Timing(config) => Handler::Timing(TimingHost::new(config.clone())),
            };
            handlers.push(handler);
        }

        let actor_handle = ActorHandle::new(
            operation_tx.clone(), 
            id.clone(), 
            theater_tx.clone()
        );
        let actor_store = ActorStore::new(id.clone(), theater_tx.clone(), actor_handle.clone());
        let mut actor_component = ActorComponent::new(config, actor_store).await.expect(
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

        // Actor handle already created above
        let mut init_state = None;
        if init {
            info!("Loading init state for actor: {:?}", id);

            // Get state from config if available
            let config_state = config.load_init_state().unwrap_or(None);

            // Merge with provided state
            init_state = crate::utils::merge_initial_states(config_state, state_bytes)
                .expect("Failed to merge initial states");

            info!("Final init state ready: {:?}", init_state.is_some());
        }

        actor_instance.store.data_mut().set_state(init_state);

        // Create a shutdown receiver for the executor
        let executor_shutdown = shutdown_controller.subscribe();
        let mut actor_executor =
            ActorExecutor::new(actor_instance, operation_rx, executor_shutdown);
        let executor_task = tokio::spawn(async move { actor_executor.run().await });

        if init {
            actor_handle
                .call_function::<(String,), ()>(
                    "ntwk:theater/actor.init".to_string(),
                    (id.to_string(),),
                )
                .await
                .expect("Failed to call init function");
        }

        let mut handler_tasks: Vec<JoinHandle<()>> = Vec::new();

        for mut handler in handlers {
            info!("Starting handler: {:?}", handler.name());
            let actor_handle = actor_handle.clone();
            let handler_shutdown = shutdown_controller.subscribe();
            let handler_task = tokio::spawn(async move {
                if let Err(e) = handler.start(actor_handle, handler_shutdown).await {
                    warn!("Handler failed: {:?}", e);
                }
            });
            handler_tasks.push(handler_task);
        }

        // Monitor parent shutdown signal and propagate
        let shutdown_controller_clone = shutdown_controller.clone();
        let mut parent_shutdown_receiver_clone = parent_shutdown_receiver;
        tokio::spawn(async move {
            debug!("Actor waiting for parent shutdown signal");
            parent_shutdown_receiver_clone.wait_for_shutdown().await;
            info!("Actor runtime received parent shutdown signal");
            debug!("Propagating shutdown signal to all handler components");
            shutdown_controller_clone.signal_shutdown();
            debug!("Shutdown signal propagated to all components");
        });

        Ok(ActorRuntime {
            actor_id: id.clone(),
            handler_tasks,
            actor_executor_task: executor_task,
            shutdown_controller,
        })
    }

    pub async fn stop(&mut self) -> Result<()> {
        info!("Initiating actor runtime shutdown");

        // Signal shutdown to all components
        info!("Signaling shutdown to all components");
        self.shutdown_controller.signal_shutdown();

        // Wait a bit for graceful shutdown
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // If any handlers are still running, abort them
        for task in self.handler_tasks.drain(..) {
            if !task.is_finished() {
                debug!("Aborting handler task that didn't shut down gracefully");
                task.abort();
            }
        }

        // Finally abort the executor if it's still running
        if !self.actor_executor_task.is_finished() {
            debug!("Aborting executor task that didn't shut down gracefully");
            self.actor_executor_task.abort();
        }

        info!("Actor runtime shutdown complete");
        Ok(())
    }
}
