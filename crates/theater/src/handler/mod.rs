use crate::actor::handle::ActorHandle;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
pub trait Handler: Send + Sync + 'static {
    async fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Result<()>;

    async fn setup_host_functions(&mut self, actor_component: &mut ActorComponent) -> Result<()>;

    async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()>;

    fn name(&self) -> &str;
}
