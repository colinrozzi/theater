use crate::actor::handle::ActorHandle;
use crate::config::permissions::*;
use crate::events::{
    environment::EnvironmentEventData, filesystem::FilesystemEventData, http::HttpEventData,
    message::MessageEventData, process::ProcessEventData, random::RandomEventData,
    runtime::RuntimeEventData, store::StoreEventData, supervisor::SupervisorEventData,
    timing::TimingEventData, ChainEventData, EventData,
};
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
use std::future::Future;
use std::pin::Pin;

/// Type alias used by handler lifecycle trait methods.
pub type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
pub trait Handler: Send + 'static {
    fn start<'a>(
        &'a mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> HandlerFuture<'a>;

    fn setup_host_functions<'a>(
        &'a mut self,
        actor_component: &'a mut ActorComponent,
    ) -> HandlerFuture<'a>;

    fn add_export_functions<'a>(
        &'a self,
        actor_instance: &'a mut ActorInstance,
    ) -> HandlerFuture<'a>;

    fn name(&self) -> &str;
}
