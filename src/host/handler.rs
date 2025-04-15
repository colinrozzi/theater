use crate::actor::handle::ActorHandle;
use crate::host::filesystem::FileSystemHost;
use crate::host::framework::HttpFramework;
use crate::host::http_client::HttpClientHost;
use crate::host::message_server::MessageServerHost;
use crate::host::process::ProcessHost;
use crate::host::runtime::RuntimeHost;
use crate::host::store::StoreHost;
use crate::host::supervisor::SupervisorHost;
use crate::host::timing::TimingHost;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;

pub enum Handler {
    MessageServer(MessageServerHost),
    FileSystem(FileSystemHost),
    HttpClient(HttpClientHost),
    HttpFramework(HttpFramework),
    Process(ProcessHost),
    Runtime(RuntimeHost),
    Supervisor(SupervisorHost),
    Store(StoreHost),
    Timing(TimingHost),
}

impl Handler {
    pub async fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        match self {
            Handler::MessageServer(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting message server")),
            Handler::FileSystem(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting filesystem")),
            Handler::HttpClient(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting http client")),
            Handler::HttpFramework(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting http framework")),
            Handler::Process(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting process handler")),
            Handler::Runtime(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting runtime")),
            Handler::Supervisor(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting supervisor")),
            Handler::Store(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting store")),
            Handler::Timing(h) => Ok(h
                .start(actor_handle, shutdown_receiver)
                .await
                .expect("Error starting timing")),
        }
    }

    pub async fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Result<()> {
        match self {
            Handler::MessageServer(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up message server host functions")),
            Handler::FileSystem(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up filesystem host functions")),
            Handler::HttpClient(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up http client host functions")),
            Handler::HttpFramework(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up http framework host functions")),
            Handler::Process(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up process host functions")),
            Handler::Runtime(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up runtime host functions")),
            Handler::Supervisor(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up supervisor host functions")),
            Handler::Store(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up store host functions")),
            Handler::Timing(h) => Ok(h
                .setup_host_functions(actor_component)
                .await
                .expect("Error setting up timing host functions")),
        }
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        match self {
            Handler::MessageServer(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to message server")),
            Handler::FileSystem(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to filesystem")),
            Handler::HttpClient(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to http client")),
            Handler::HttpFramework(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to http framework")),
            Handler::Process(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to process handler")),
            Handler::Runtime(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to runtime")),
            Handler::Supervisor(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to supervisor")),
            Handler::Store(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to store")),
            Handler::Timing(handler) => Ok(handler
                .add_export_functions(actor_instance)
                .await
                .expect("Error adding functions to timing")),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Handler::MessageServer(_) => "message-server",
            Handler::FileSystem(_) => "filesystem",
            Handler::HttpClient(_) => "http-client",
            Handler::HttpFramework(_) => "http-framework",
            Handler::Process(_) => "process",
            Handler::Runtime(_) => "runtime",
            Handler::Supervisor(_) => "supervisor",
            Handler::Store(_) => "store",
            Handler::Timing(_) => "timing",
        }
    }
}
