// okay what are we doing here.
// we want to wrap the existing SimpleHandler host functions so that they can be used with
// HostHandler

use theater::handler::HostHandler;
use theater::host::SimpleHandler;

pub struct SimpleHandlerWrapper;

impl HostHandler for SimpleHandlerWrapper {
    fn start(
        &mut self,
        actor_handle: theater::actor::handle::ActorHandle,
        shutdown_receiver: theater::shutdown::ShutdownReceiver,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        SimpleHandler::start(self, actor_handle, shutdown_receiver)
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
