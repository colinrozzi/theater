use crate::actor::handle::ActorHandle;
use crate::events::EventPayload;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Shared reference to an actor instance for handlers that need direct store access
pub type SharedActorInstance<E> = Arc<RwLock<Option<ActorInstance<E>>>>;

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

        debug!("setup_handlers called");
        debug!("Component imports: {:?}", component_imports.iter().map(|(n, _)| n).collect::<Vec<_>>());
        debug!("Component exports: {:?}", component_exports.iter().map(|(n, _)| n).collect::<Vec<_>>());
        debug!("Number of registered handlers: {}", self.handlers.len());

        let mut active_handlers = Vec::new();

        for handler in &self.handlers {
            debug!("Checking handler '{}' - imports: {:?}, exports: {:?}",
                   handler.name(), handler.imports(), handler.exports());

            // Check if handler's imports match component's imports
            // Handler imports can be comma-separated (e.g., "wasi:clocks/wall-clock@0.2.3,wasi:io/poll@0.2.3")
            let imports_match = handler.imports().map_or(false, |imports_str| {
                // Split comma-separated imports and check if any match
                imports_str.split(',')
                    .map(|s| s.trim())
                    .any(|handler_import| {
                        let matches = component_imports.iter().any(|(name, _)| name == handler_import);
                        debug!("Checking import '{}' against component imports: {}", handler_import, matches);
                        matches
                    })
            });

            // Check if handler's exports match component's exports
            let exports_match = handler.exports().map_or(false, |exports_str| {
                // Split comma-separated exports and check if any match
                exports_str.split(',')
                    .map(|s| s.trim())
                    .any(|handler_export| {
                        let matches = component_exports.iter().any(|(name, _)| name == handler_export);
                        debug!("Checking export '{}' against component exports: {}", handler_export, matches);
                        matches
                    })
            });

            let needs_this_handler = imports_match || exports_match;
            debug!("Handler '{}': imports_match={}, exports_match={}, needs_this_handler={}",
                   handler.name(), imports_match, exports_match, needs_this_handler);

            if needs_this_handler {
                active_handlers.push(handler.create_instance());
                info!("Activated handler '{}'", handler.name());
            }
        }

        debug!("setup_handlers returning {} handlers", active_handlers.len());
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
        actor_instance: SharedActorInstance<E>,
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
