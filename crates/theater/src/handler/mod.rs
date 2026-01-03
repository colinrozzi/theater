use crate::actor::handle::ActorHandle;
use crate::events::EventPayload;
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Shared reference to an actor instance for handlers that need direct store access
pub type SharedActorInstance<E> = Arc<RwLock<Option<ActorInstance<E>>>>;

/// Context passed to handlers during setup, tracking which imports are already satisfied
#[derive(Debug, Clone, Default)]
pub struct HandlerContext {
    /// Set of imports that have already been registered by other handlers
    pub satisfied_imports: HashSet<String>,
}

impl HandlerContext {
    pub fn new() -> Self {
        Self {
            satisfied_imports: HashSet::new(),
        }
    }

    /// Check if an import is already satisfied
    pub fn is_satisfied(&self, import: &str) -> bool {
        self.satisfied_imports.contains(import)
    }

    /// Mark an import as satisfied
    pub fn mark_satisfied(&mut self, import: &str) {
        self.satisfied_imports.insert(import.to_string());
    }

    /// Mark multiple imports as satisfied
    pub fn mark_all_satisfied(&mut self, imports: &[String]) {
        for import in imports {
            self.satisfied_imports.insert(import.clone());
        }
    }
}

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
        let component_imports: HashSet<String> = actor_component
            .import_types
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let component_exports: HashSet<String> = actor_component
            .export_types
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        debug!("setup_handlers called");
        debug!("Component imports: {:?}", component_imports);
        debug!("Component exports: {:?}", component_exports);
        debug!("Number of registered handlers: {}", self.handlers.len());

        let mut active_handlers = Vec::new();

        for handler in &self.handlers {
            let handler_imports = handler.imports();
            let handler_exports = handler.exports();

            debug!(
                "Checking handler '{}' - imports: {:?}, exports: {:?}",
                handler.name(),
                handler_imports,
                handler_exports
            );

            // Check if any of handler's imports match component's imports
            // None means "match all imports" (useful for catch-all handlers like ReplayHandler)
            let imports_match = match handler_imports.as_ref() {
                None => {
                    debug!("Handler '{}' has None imports - matches all", handler.name());
                    true
                }
                Some(imports) => imports.iter().any(|import| {
                    let matches = component_imports.contains(import);
                    debug!(
                        "Checking import '{}' against component imports: {}",
                        import, matches
                    );
                    matches
                }),
            };

            // Check if any of handler's exports match component's exports
            let exports_match = handler_exports
                .as_ref()
                .map_or(false, |exports| {
                    exports.iter().any(|export| {
                        let matches = component_exports.contains(export);
                        debug!(
                            "Checking export '{}' against component exports: {}",
                            export, matches
                        );
                        matches
                    })
                });

            let needs_this_handler = imports_match || exports_match;
            debug!(
                "Handler '{}': imports_match={}, exports_match={}, needs_this_handler={}",
                handler.name(),
                imports_match,
                exports_match,
                needs_this_handler
            );

            if needs_this_handler {
                active_handlers.push(handler.create_instance());
                info!("Activated handler '{}'", handler.name());
            }
        }

        debug!(
            "setup_handlers returning {} handlers",
            active_handlers.len()
        );
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

    /// Set up host functions for this handler.
    ///
    /// The `ctx` parameter provides information about which imports have already been
    /// satisfied by other handlers. Handlers should check `ctx.is_satisfied(import)`
    /// before registering an interface, and call `ctx.mark_satisfied(import)` after
    /// successfully registering one.
    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
        ctx: &mut HandlerContext,
    ) -> Result<()>;

    fn add_export_functions(&self, actor_instance: &mut ActorInstance<E>) -> Result<()>;

    fn name(&self) -> &str;

    /// Returns the list of imports this handler can satisfy.
    /// Used for matching handlers to components that need these imports.
    fn imports(&self) -> Option<Vec<String>>;

    /// Returns the list of exports this handler expects from the component.
    /// Used for matching handlers to components that export these interfaces.
    fn exports(&self) -> Option<Vec<String>>;
}
