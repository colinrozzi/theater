use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::chain::ChainEvent;
use crate::composite_bridge::{CompositeInstance, HostLinkerBuilder, LinkerError};
use crate::config::actor_manifest::HandlerConfig;
use crate::shutdown::ShutdownReceiver;
use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared reference to an actor instance for handlers that need direct store access
pub type SharedActorInstance = Arc<RwLock<Option<CompositeInstance>>>;

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

pub struct HandlerRegistry {
    handlers: Vec<Box<dyn Handler>>,
    /// Optional replay chain events - set when in replay mode.
    /// Handlers can use this to replay recorded events instead of running normally.
    replay_chain: Option<Vec<ChainEvent>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            replay_chain: None,
        }
    }

    /// Set the replay chain for this registry.
    /// When set, handlers can check for this to enable replay mode.
    pub fn set_replay_chain(&mut self, chain: Vec<ChainEvent>) {
        self.replay_chain = Some(chain);
    }

    /// Get the replay chain if set.
    pub fn replay_chain(&self) -> Option<&Vec<ChainEvent>> {
        self.replay_chain.as_ref()
    }

    /// Take ownership of the replay chain (removes it from the registry).
    pub fn take_replay_chain(&mut self) -> Option<Vec<ChainEvent>> {
        self.replay_chain.take()
    }

    /// Check if this registry is in replay mode.
    pub fn is_replay_mode(&self) -> bool {
        self.replay_chain.is_some()
    }

    pub fn register<H: Handler>(&mut self, handler: H) {
        self.handlers.push(Box::new(handler));
    }

    /// Prepend a handler to the beginning of the registry.
    /// This is useful when you want a handler to be checked first
    /// (e.g., ReplayHandler should intercept imports before other handlers).
    pub fn prepend<H: Handler>(&mut self, handler: H) {
        self.handlers.insert(0, Box::new(handler));
    }
}

impl Clone for HandlerRegistry {
    fn clone(&self) -> Self {
        let mut new_registry = HandlerRegistry::new();
        for handler in &self.handlers {
            // Each handler creates a fresh instance of itself (no config override)
            new_registry.handlers.push(handler.create_instance(None));
        }
        // Preserve replay chain if set
        if let Some(chain) = &self.replay_chain {
            new_registry.replay_chain = Some(chain.clone());
        }
        new_registry
    }
}

impl HandlerRegistry {
    /// Get all handlers for Composite instantiation.
    ///
    /// Unlike `setup_handlers` which filters based on wasmtime component metadata,
    /// this returns all registered handlers. Composite will fail at instantiation
    /// if required imports aren't satisfied.
    pub fn get_handlers(&self) -> Vec<Box<dyn Handler>> {
        self.handlers
            .iter()
            .map(|h| h.create_instance(None))
            .collect()
    }

    /// Clone the registry and apply per-actor configs from a manifest.
    ///
    /// For each handler config in the list, finds the matching handler by name
    /// and creates a new instance with that config.
    pub fn clone_with_configs(&self, configs: &[HandlerConfig]) -> Self {
        let mut new_registry = HandlerRegistry::new();
        for handler in &self.handlers {
            // Check if there's a config for this handler
            let matching_config = configs
                .iter()
                .find(|c| c.handler_name() == handler.name());
            new_registry
                .handlers
                .push(handler.create_instance(matching_config));
        }
        // Preserve replay chain if set
        if let Some(chain) = &self.replay_chain {
            new_registry.replay_chain = Some(chain.clone());
        }
        new_registry
    }
}

/// Trait describing the lifecycle hooks every handler must implement.
///
/// External handler crates can implement this trait and register their handlers
/// with the Theater runtime without depending on the concrete `Handler` enum.
///
/// ## Composite Migration
///
/// For handlers migrating to Composite's Graph ABI runtime, implement these methods:
/// - `setup_host_functions_composite()` - Register host functions using `HostLinkerBuilder`
/// - `register_exports_composite()` - Register export function metadata
///
/// These have default implementations that do nothing, allowing gradual migration.
pub trait Handler: Send + Sync + 'static {
    /// Create a new instance of this handler, optionally with a config from the manifest.
    ///
    /// If `config` is `Some` and matches this handler's type, creates a new instance
    /// with that config. Otherwise, clones the current instance.
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler>;

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;

    /// Set up host functions for this handler (Composite Graph ABI runtime).
    ///
    /// This is the new method for Composite integration. Handlers should register
    /// their host functions using the `HostLinkerBuilder`:
    ///
    /// ```ignore
    /// fn setup_host_functions_composite(
    ///     &mut self,
    ///     builder: &mut HostLinkerBuilder<'_, ActorStore>,
    ///     ctx: &mut HandlerContext,
    /// ) -> Result<(), LinkerError> {
    ///     if ctx.is_satisfied("my:interface") {
    ///         return Ok(());
    ///     }
    ///
    ///     builder.interface("my:interface")?
    ///         .func_typed("my_function", |ctx: &mut Ctx<'_, ActorStore>, input: String| {
    ///             // handle the call
    ///             "result".to_string()
    ///         })?;
    ///
    ///     ctx.mark_satisfied("my:interface");
    ///     Ok(())
    /// }
    /// ```
    ///
    /// Default implementation does nothing, allowing gradual migration.
    fn setup_host_functions_composite(
        &mut self,
        _builder: &mut HostLinkerBuilder<'_, ActorStore>,
        _ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        // Default: do nothing - handlers opt-in by overriding
        Ok(())
    }

    /// Register export function metadata for Composite instances.
    ///
    /// This records which export functions this handler expects from the actor.
    /// The actual function calls are made through `CompositeInstance::call_function()`.
    ///
    /// Default implementation does nothing, allowing gradual migration.
    fn register_exports_composite(&self, _instance: &mut CompositeInstance) -> Result<()> {
        // Default: do nothing - handlers opt-in by overriding
        Ok(())
    }

    fn name(&self) -> &str;

    /// Returns the list of imports this handler can satisfy.
    /// Used for matching handlers to components that need these imports.
    fn imports(&self) -> Option<Vec<String>>;

    /// Returns the list of exports this handler expects from the component.
    /// Used for matching handlers to components that export these interfaces.
    fn exports(&self) -> Option<Vec<String>>;

    /// Returns true if this handler supports Composite's Graph ABI runtime.
    ///
    /// Handlers that override `setup_host_functions_composite()` should
    /// return `true` here. This is used by ActorRuntime to determine
    /// which runtime to use.
    fn supports_composite(&self) -> bool {
        false
    }
}
