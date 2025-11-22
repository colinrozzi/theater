use crate::actor::handle::ActorHandle;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

pub struct HandlerRegistry {
    handlers: Vec<Box<dyn Handler>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn register<H: Handler>(&mut self, handler: H) {
        self.handlers.push(Box::new(handler));
    }

    pub fn setup_handlers(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Vec<Box<dyn Handler>> {
        unimplemented!()
    }
}

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
pub trait Handler: Send + Sync + 'static {
    fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn name(&self) -> &str;

    fn imports(&self) -> Option<String>;

    fn exports(&self) -> Option<String>;
}
