use crate::actor::handle::ActorHandle;
use crate::events::EventPayload;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

pub struct HandlerRegistry<E>
where
    E: EventPayload + Clone,
{
    handlers: Vec<Box<dyn Handler<E>>>,
}

impl<E> HandlerRegistry<E>
where
    E: EventPayload + Clone,
{
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn register<H: Handler<E>>(&mut self, handler: H) {
        self.handlers.push(Box::new(handler));
    }

    pub fn setup_handlers(
        &mut self,
        actor_component: &mut ActorComponent<E>,
    ) -> Vec<Box<dyn Handler<E>>> {
        let component_imports = actor_component.import_types.clone(); // What the component imports
        let component_exports = actor_component.export_types.clone(); // What the component exports

        let mut active_handlers = Vec::new();

        for handler in &self.handlers {
            let needs_this_handler = handler.imports().map_or(false, |import| {
                component_imports.iter().any(|(name, _)| name == &import)
            }) || handler.exports().map_or(false, |export| {
                component_exports.iter().any(|(name, _)| name == &export)
            });

            if needs_this_handler {
                active_handlers.push(handler.create_instance());
            }
        }

        active_handlers
    }
}

impl<E> Clone for HandlerRegistry<E>
where
    E: EventPayload + Clone,
{
    fn clone(&self) -> Self {
        let mut new_registry = HandlerRegistry::new();
        for handler in &self.handlers {
            // Each handler creates a fresh instance of itself
            new_registry.handlers.push(handler.create_instance());
        }
        new_registry
    }
}

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
pub trait Handler<E>: Send + Sync + 'static
where
    E: EventPayload + Clone,
{
    fn create_instance(&self) -> Box<dyn Handler<E>>;

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
    ) -> Result<()>;

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance<E>,
    ) -> Result<()>;

    fn name(&self) -> &str;

    fn imports(&self) -> Option<String>;

    fn exports(&self) -> Option<String>;
}
