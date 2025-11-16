use crate::actor::handle::ActorHandle;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use std::future::Future;

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
pub trait Handler: Send + Sync + 'static {
    fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> impl Future<Output = Result<()>> + Send;

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> impl Future<Output = Result<()>> + Send;

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> impl Future<Output = Result<()>> + Send;

    fn name(&self) -> &str;
}

pub trait HostHandler: Send + Sync + 'static + Sized + Clone {
    fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> impl Future<Output = Result<()>> + Send;

    fn name(&self) -> &str;

    fn setup_handlers(
        &self,
        actor_component: &mut ActorComponent,
    ) -> impl Future<Output = Result<Vec<Self>>> + Send;
}
