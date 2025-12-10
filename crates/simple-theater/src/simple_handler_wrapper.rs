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
        async move {
            match self {
                SimpleHandlerWrapper::MessageServer(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Environment(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::FileSystem(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::HttpClient(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::HttpFramework(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Process(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Runtime(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Supervisor(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Store(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Timing(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
                SimpleHandlerWrapper::Random(handler) => {
                    handler.start(actor_handle, shutdown_receiver).await
                }
            }
        }
    }

    fn name(&self) -> &str {
        match self {
            SimpleHandlerWrapper::MessageServer(_handler) => "message_server",
            SimpleHandlerWrapper::Environment(_handler) => "environment",
            SimpleHandlerWrapper::FileSystem(_handler) => "filesystem",
            SimpleHandlerWrapper::HttpClient(_handler) => "http_client",
            SimpleHandlerWrapper::HttpFramework(_handler) => "http_framework",
            SimpleHandlerWrapper::Process(_handler) => "process",
            SimpleHandlerWrapper::Runtime(_handler) => "runtime",
            SimpleHandlerWrapper::Supervisor(_handler) => "supervisor",
            SimpleHandlerWrapper::Store(_handler) => "store",
            SimpleHandlerWrapper::Timing(_handler) => "timing",
            SimpleHandlerWrapper::Random(_handler) => "random",
        }
    }

    fn get_handlers(
        &self,
        actor_component: &mut theater::wasm::ActorComponent,
    ) -> Vec<HostHandler> {
        let mut handlers = Vec::new();
        for handler in self {
            if handler.is_required(actor_component) {
                handlers.push(handler.clone());
            }
        }
        handlers
    }

    fn setup_handlers(
        &self,
        actor_component: &mut theater::wasm::ActorComponent,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        let fut = async move {
            match self {
                SimpleHandlerWrapper::MessageServer(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Environment(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::FileSystem(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::HttpClient(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::HttpFramework(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Process(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Runtime(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Supervisor(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Store(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Timing(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
                SimpleHandlerWrapper::Random(handler) => {
                    handler.setup_host_functions(actor_component).await
                }
            }
        };
    }
}
