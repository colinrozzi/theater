use crate::actor::handle::ActorHandle;
use crate::config::permissions::*;
use crate::events::{
    environment::EnvironmentEventData, filesystem::FilesystemEventData, http::HttpEventData,
    message::MessageEventData, process::ProcessEventData, random::RandomEventData,
    runtime::RuntimeEventData, store::StoreEventData, supervisor::SupervisorEventData,
    timing::TimingEventData, ChainEventData, EventData,
};
use crate::handler::Handler;
use crate::host::environment::EnvironmentHost;
use crate::host::filesystem::FileSystemHost;
use crate::host::framework::HttpFramework;
use crate::host::http_client::HttpClientHost;
use crate::host::message_server::MessageServerHost;
use crate::host::process::ProcessHost;
use crate::host::random::RandomHost;
use crate::host::runtime::RuntimeHost;
use crate::host::store::StoreHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::timing::TimingHost;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;

pub enum SimpleHandler {
    MessageServer(MessageServerHost, Option<MessageServerPermissions>),
    Environment(EnvironmentHost, Option<EnvironmentPermissions>),
    FileSystem(FileSystemHost, Option<FileSystemPermissions>),
    HttpClient(HttpClientHost, Option<HttpClientPermissions>),
    HttpFramework(HttpFramework, Option<HttpFrameworkPermissions>),
    Process(ProcessHost, Option<ProcessPermissions>),
    Runtime(RuntimeHost, Option<RuntimePermissions>),
    Supervisor(SupervisorHost, Option<SupervisorPermissions>),
    Store(StoreHost, Option<StorePermissions>),
    Timing(TimingHost, Option<TimingPermissions>),
    Random(RandomHost, Option<RandomPermissions>),
}

impl Handler for SimpleHandler {
    fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        Box::pin(self.start(actor_handle, shutdown_receiver))
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        Box::pin(self.setup_host_functions(actor_component))
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        Box::pin(self.add_export_functions(actor_instance))
    }

    fn name(&self) -> &str {
        self.name()
    }
}

impl SimpleHandler {
    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        match self {
            SimpleHandler::MessageServer(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting message server")),
            SimpleHandler::Environment(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting environment handler")),
            SimpleHandler::FileSystem(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting filesystem")),
            SimpleHandler::HttpClient(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting http client")),
            SimpleHandler::HttpFramework(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting http framework")),
            SimpleHandler::Process(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting process handler")),
            SimpleHandler::Runtime(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting runtime")),
            SimpleHandler::Supervisor(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting supervisor")),
            SimpleHandler::Store(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting store")),
            SimpleHandler::Timing(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting timing")),
            SimpleHandler::Random(h, _) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting random")),
        }
    }

    pub async fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Result<()> {
        match self {
            SimpleHandler::MessageServer(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up message server host functions")),
            SimpleHandler::Environment(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up environment host functions")),
            SimpleHandler::FileSystem(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up filesystem host functions")),
            SimpleHandler::HttpClient(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up http client host functions")),
            SimpleHandler::HttpFramework(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up http framework host functions")),
            SimpleHandler::Process(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up process host functions")),
            SimpleHandler::Runtime(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up runtime host functions")),
            SimpleHandler::Supervisor(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up supervisor host functions")),
            SimpleHandler::Store(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up store host functions")),
            SimpleHandler::Timing(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up timing host functions")),
            SimpleHandler::Random(h, _) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up random host functions")),
        }
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        match self {
            SimpleHandler::MessageServer(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to message server: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "message-server-export-setup".to_string(),
                                data: EventData::Message(MessageEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Environment(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg =
                            format!("Error adding functions to environment handler: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "environment-export-setup".to_string(),
                                data: EventData::Environment(
                                    EnvironmentEventData::HandlerSetupError {
                                        error: error_msg.clone(),
                                        step: "add_export_functions".to_string(),
                                    },
                                ),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::FileSystem(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to filesystem: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "filesystem-export-setup".to_string(),
                                data: EventData::Filesystem(
                                    FilesystemEventData::HandlerSetupError {
                                        error: error_msg.clone(),
                                        step: "add_export_functions".to_string(),
                                    },
                                ),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::HttpClient(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to http client: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "http-client-export-setup".to_string(),
                                data: EventData::Http(HttpEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::HttpFramework(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to http framework: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "http-framework-export-setup".to_string(),
                                data: EventData::Http(HttpEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Process(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to process handler: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "process-export-setup".to_string(),
                                data: EventData::Process(ProcessEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Runtime(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to runtime: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "runtime-export-setup".to_string(),
                                data: EventData::Runtime(RuntimeEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Supervisor(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to supervisor: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "supervisor-export-setup".to_string(),
                                data: EventData::Supervisor(
                                    SupervisorEventData::HandlerSetupError {
                                        error: error_msg.clone(),
                                        step: "add_export_functions".to_string(),
                                    },
                                ),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Store(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to store: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "store-export-setup".to_string(),
                                data: EventData::Store(StoreEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Timing(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to timing: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "timing-export-setup".to_string(),
                                data: EventData::Timing(TimingEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
            SimpleHandler::Random(handler, _) => {
                match handler.add_export_functions(actor_instance).await {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_msg = format!("Error adding functions to random: {}", e);
                        actor_instance
                            .actor_component
                            .actor_store
                            .record_event(ChainEventData {
                                event_type: "random-export-setup".to_string(),
                                data: EventData::Random(RandomEventData::HandlerSetupError {
                                    error: error_msg.clone(),
                                    step: "add_export_functions".to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(error_msg),
                            });
                        Err(e)
                    }
                }
            }
        }
    }

    pub fn name(&self) -> &str {
        match self {
            SimpleHandler::MessageServer(_, _) => "message-server",
            SimpleHandler::Environment(_, _) => "environment",
            SimpleHandler::FileSystem(_, _) => "filesystem",
            SimpleHandler::HttpClient(_, _) => "http-client",
            SimpleHandler::HttpFramework(_, _) => "http-framework",
            SimpleHandler::Process(_, _) => "process",
            SimpleHandler::Runtime(_, _) => "runtime",
            SimpleHandler::Supervisor(_, _) => "supervisor",
            SimpleHandler::Store(_, _) => "store",
            SimpleHandler::Timing(_, _) => "timing",
            SimpleHandler::Random(_, _) => "random",
        }
    }
}
