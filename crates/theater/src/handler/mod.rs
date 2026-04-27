use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::chain::ChainEvent;
use crate::config::actor_manifest::HandlerConfig;
use crate::id::TheaterId;
use crate::pack_bridge::{HostLinkerBuilder, LinkerError, PackInstance, TypeHash};
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

/// Shared reference to an actor instance for handlers that need direct store access
pub type SharedActorInstance = Arc<RwLock<Option<PackInstance>>>;

/// Context passed to handlers during setup, tracking which imports are already satisfied
/// and providing access to the shutdown controller for handlers that need it.
#[derive(Debug, Clone)]
pub struct HandlerContext {
    /// Set of imports that have already been registered by other handlers
    pub satisfied_imports: HashSet<String>,
    /// The actor ID for the actor being set up
    pub actor_id: Option<TheaterId>,
    /// Shutdown controller - handlers can subscribe to get shutdown signals
    pub shutdown_controller: Option<ShutdownController>,
}

impl Default for HandlerContext {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlerContext {
    pub fn new() -> Self {
        Self {
            satisfied_imports: HashSet::new(),
            actor_id: None,
            shutdown_controller: None,
        }
    }

    /// Create a new context with a shutdown controller
    pub fn with_shutdown_controller(shutdown_controller: ShutdownController) -> Self {
        Self {
            satisfied_imports: HashSet::new(),
            actor_id: None,
            shutdown_controller: Some(shutdown_controller),
        }
    }

    /// Get a shutdown receiver from the controller, if available
    pub fn subscribe_shutdown(&mut self) -> Option<ShutdownReceiver> {
        self.shutdown_controller.as_mut().map(|c| c.subscribe())
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
            let matching_config = configs.iter().find(|c| c.handler_name() == handler.name());
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
/// ## Handler Lifecycle
///
/// 1. `create_instance()` - Clone/create handler instance with optional config
/// 2. `setup_host_functions_composite()` - Register host functions (sync, during instantiation)
///    - HandlerContext provides shutdown_controller for handlers that need early access
/// 3. `init()` - Synchronous critical initialization (called before actor can receive calls)
/// 4. `run()` - Async runtime loop (spawned as background task)
///
/// ## Composite Migration
///
/// For handlers migrating to Composite's Graph ABI runtime, implement:
/// - `setup_host_functions_composite()` - Register host functions using `HostLinkerBuilder`
///
/// Export discovery is automatic via Pack's embedded `__pack_types` metadata,
/// so handlers no longer need to manually register exports.
pub trait Handler: Send + Sync + 'static {
    /// Create a new instance of this handler, optionally with a config from the manifest.
    ///
    /// If `config` is `Some` and matches this handler's type, creates a new instance
    /// with that config. Otherwise, clones the current instance.
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler>;

    /// Synchronous initialization called BEFORE the actor can receive any calls.
    ///
    /// This is the place for critical setup that must complete before host functions
    /// can be used. For example, storing actor handles that host functions depend on.
    ///
    /// Note: If a handler needs a ShutdownReceiver for host functions, it should
    /// subscribe via `ctx.subscribe_shutdown()` during `setup_host_functions_composite()`.
    ///
    /// This runs synchronously - do NOT do any async work here.
    /// Default implementation does nothing.
    fn init(&mut self, _actor_handle: ActorHandle, _actor_instance: SharedActorInstance) {
        // Default: no-op
    }

    /// Async runtime loop that runs for the handler's lifetime.
    ///
    /// This is spawned as a background task AFTER init() completes and the actor
    /// is ready to receive calls. Use this for event loops, message consumption,
    /// or any long-running async operations.
    ///
    /// The `event_rx` parameter receives chain events as they're recorded. Most handlers
    /// can ignore this, but ReplayHandler uses it for streaming hash verification.
    ///
    /// Default implementation just waits for shutdown.
    fn run(
        &mut self,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: broadcast::Receiver<ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            Ok(())
        })
    }

    /// Initialize and run the handler.
    ///
    /// The runtime calls init() synchronously, then spawns run() as a background task.
    /// Most handlers should override init() and/or run() rather than this method.
    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        event_rx: broadcast::Receiver<ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        // Call init synchronously first
        self.init(actor_handle, actor_instance);
        // Then return the run future
        self.run(shutdown_receiver, event_rx)
    }

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

    fn name(&self) -> &str;

    /// Returns the list of imports this handler can satisfy.
    /// Used for matching handlers to components that need these imports.
    fn imports(&self) -> Option<Vec<String>>;

    /// Returns the list of exports this handler expects from the component.
    /// Used for matching handlers to components that export these interfaces.
    fn exports(&self) -> Option<Vec<String>>;

    /// Returns the interface hashes for each interface this handler provides.
    ///
    /// Interface hashes enable O(1) compatibility checking between handlers and
    /// components. Two interfaces are compatible if their hashes match.
    ///
    /// Handlers compute these hashes from `.pact` files using `InterfaceImpl::from_pact()`:
    ///
    /// ```ignore
    /// use pack::{parse_pact, InterfaceImpl, TypeHash};
    ///
    /// const MY_PACT: &str = include_str!("../../../pact/my-interface.pact");
    ///
    /// fn my_interface() -> InterfaceImpl {
    ///     let pact = parse_pact(MY_PACT).expect("embedded pact should be valid");
    ///     InterfaceImpl::from_pact(&pact)
    /// }
    ///
    /// fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
    ///     self.interfaces()
    ///         .iter()
    ///         .map(|i| (i.name().to_string(), i.hash()))
    ///         .collect()
    /// }
    /// ```
    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        vec![]
    }

    /// Returns the InterfaceImpl declarations for each interface this handler provides.
    ///
    /// This enables subset hash computation for partial interface matching.
    /// When an actor imports only some functions from an interface, the runtime
    /// can compute a subset hash to verify compatibility.
    ///
    /// Handlers should load interfaces from `.pact` files:
    ///
    /// ```ignore
    /// use pack::{parse_pact, InterfaceImpl};
    ///
    /// const MY_PACT: &str = include_str!("../../../pact/my-interface.pact");
    ///
    /// fn interfaces(&self) -> Vec<InterfaceImpl> {
    ///     let pact = parse_pact(MY_PACT).expect("embedded pact should be valid");
    ///     vec![InterfaceImpl::from_pact(&pact)]
    /// }
    /// ```
    fn interfaces(&self) -> Vec<crate::pack_bridge::InterfaceImpl> {
        vec![]
    }

    /// Returns true if this handler supports Composite's Graph ABI runtime.
    ///
    /// Handlers that override `setup_host_functions_composite()` should
    /// return `true` here. This is used by ActorRuntime to determine
    /// which runtime to use.
    fn supports_composite(&self) -> bool {
        false
    }
}
