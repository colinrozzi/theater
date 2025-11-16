// okay what are we doing here.
// we want to wrap the existing SimpleHandler host functions so that they can be used with
// HostHandler

use anyhow::Result;
use std::future::Future;
use theater::handler::HostHandler;
use theater::host::SimpleHandler;
use theater::host::{
    environment::EnvironmentHost, filesystem::FileSystemHost, framework::HttpFramework,
    http_client::HttpClientHost, message_server::MessageServerHost, process::ProcessHost,
    random::RandomHost, runtime::RuntimeHost, store::StoreHost, supervisor::SupervisorHost,
    timing::TimingHost,
};

pub enum SimpleHandlerWrapper {
    MessageServer(MessageServerHost),
    Environment(EnvironmentHost),
    FileSystem(FileSystemHost),
    HttpClient(HttpClientHost),
    HttpFramework(HttpFramework),
    Process(ProcessHost),
    Runtime(RuntimeHost),
    Supervisor(SupervisorHost),
    Store(StoreHost),
    Timing(TimingHost),
    Random(RandomHost),
}

impl HostHandler for SimpleHandlerWrapper {
    fn start(
        &mut self,
        actor_handle: theater::actor::handle::ActorHandle,
        shutdown_receiver: theater::shutdown::ShutdownReceiver,
    ) -> impl Future<Output = Result<()>> + Send {
        match self {
            SimpleHandlerWrapper::MessageServer(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::Environment(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::FileSystem(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::HttpClient(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::HttpFramework(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::Process(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::Runtime(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::Supervisor(handler) => {
                handler.start(actor_handle, shutdown_receiver)
            }
            SimpleHandlerWrapper::Store(handler) => handler.start(actor_handle, shutdown_receiver),
            SimpleHandlerWrapper::Timing(handler) => handler.start(actor_handle, shutdown_receiver),
            SimpleHandlerWrapper::Random(handler) => handler.start(actor_handle, shutdown_receiver),
        }
    }

    fn name(&self) -> &str {
        SimpleHandler::name(self)
    }

    fn setup_handlers(
        &self,
        actor_component: &mut theater::wasm::ActorComponent,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<Self>>> + Send {
        SimpleHandler::setup_host_functions(self, actor_component)
            .map(|res| res.map(|_| vec![Self]))
    }
}
