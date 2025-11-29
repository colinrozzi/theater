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

impl Clone for HandlerRegistry {
    fn clone(&self) -> Self {
        let mut new_registry = HandlerRegistry::new();
        for handler in &self.handlers {
            // Note: This requires Handler to implement Clone, which may not be possible.
            // This is just a placeholder to illustrate the idea.
            // In practice, you might need a different approach to clone handlers.
            // new_registry.register(handler.clone());
            let new_handler = handler.new();
            new_registry.register(new_handler);
        }
    }
}

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
pub trait Handler: Send + Sync + 'static {
    fn new(&self) -> Handler;

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
