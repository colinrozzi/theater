use crate::actor::handle::ActorHandle;
use crate::chain::ChainEvent;
use crate::config::actor_manifest::HandlerConfig;
use crate::id::TheaterId;
use crate::pack_bridge::{PackInstance, TypeHash};
use crate::shutdown::{ShutdownController, ShutdownReceiver};
use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

/// Receiver handlers use to observe their actor's chain events. Each
/// handler gets its own mpsc; the runtime registers the matching sender
/// on the chain before invoking `setup`. Most handlers ignore this
/// receiver — the replay handler is the only consumer that actively
/// reads it for streaming hash verification.
pub type HandlerEventReceiver = mpsc::Receiver<(TheaterId, ChainEvent)>;

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
    /// The theater command channel — captured by host functions that need to
    /// talk to the runtime (spawn, shutdown, message routing, …). The runtime
    /// sets this before invoking `register_host_functions`.
    pub theater_tx: Option<mpsc::UnboundedSender<crate::messages::TheaterCommand>>,
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
            theater_tx: None,
            shutdown_controller: None,
        }
    }

    /// Create a new context with a shutdown controller
    pub fn with_shutdown_controller(shutdown_controller: ShutdownController) -> Self {
        Self {
            satisfied_imports: HashSet::new(),
            actor_id: None,
            theater_tx: None,
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

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
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

    /// Clone the registry and apply per-actor configs + granted permissions.
    ///
    /// For each handler, creates a fresh instance with its matching config, then
    /// bakes in the actor's effective permissions (via `set_permissions`) so the
    /// runtime-side capability gate has the actor's grant. `permissions` is the
    /// effective `HandlerPermission` computed from the manifest's policy; `None`
    /// means no grants (default-deny for the gated handlers).
    pub fn clone_with_configs(
        &self,
        configs: &[HandlerConfig],
        permissions: Option<&crate::config::permissions::HandlerPermission>,
    ) -> Self {
        let mut new_registry = HandlerRegistry::new();
        for handler in &self.handlers {
            let matching_config = configs.iter().find(|c| c.type_name() == handler.name());
            let mut instance = handler.create_instance(matching_config);
            instance.set_permissions(permissions);
            new_registry.handlers.push(instance);
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

    /// Bake the actor's granted permissions into this (freshly created) instance.
    ///
    /// The registry calls this right after `create_instance`, per actor, with the
    /// actor's effective `HandlerPermission`. Handlers that gate on permissions
    /// (supervisor, runtime) extract their own slice and store it; handlers that
    /// don't need nothing — the default is a no-op. This is how the runtime-side
    /// capability gate receives each actor's grant (default-deny when absent).
    fn set_permissions(
        &mut self,
        _permissions: Option<&crate::config::permissions::HandlerPermission>,
    ) {
    }

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
        _event_rx: HandlerEventReceiver,
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
        event_rx: HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        // Call init synchronously first
        self.init(actor_handle, actor_instance);
        // Then return the run future
        self.run(shutdown_receiver, event_rx)
    }

    /// Register this handler's host functions on the actor's [`HostImports`].
    ///
    /// The capture-based engine (packr-core) resolves a guest's imports from the
    /// [`HostImports`] registry. Each host function is a closure that *captures*
    /// whatever host state it needs (the theater command channel, the actor id,
    /// per-handler capabilities) — nothing is threaded through the engine. Use
    /// [`crate::pack_bridge::result_host_fn`] for pact `result<..>` returns and
    /// [`crate::pack_bridge::plain_host_fn`] for plain-value returns.
    ///
    /// ```ignore
    /// fn register_host_functions(
    ///     &mut self,
    ///     imports: &mut packr_core::HostImports,
    ///     ctx: &mut HandlerContext,
    /// ) -> anyhow::Result<()> {
    ///     if ctx.is_satisfied("my:interface") {
    ///         return Ok(());
    ///     }
    ///     let theater_tx = ctx.theater_tx.clone().unwrap();
    ///     let id = ctx.actor_id.unwrap();
    ///     imports.define(
    ///         "my:interface",
    ///         "my_function",
    ///         crate::pack_bridge::result_host_fn(move |input: Value| {
    ///             let theater_tx = theater_tx.clone();
    ///             async move { Ok(Ok(Value::String("result".to_string()))) }
    ///         }),
    ///     );
    ///     ctx.mark_satisfied("my:interface");
    ///     Ok(())
    /// }
    /// ```
    ///
    /// Default implementation does nothing, allowing gradual migration.
    fn register_host_functions(
        &mut self,
        _imports: &mut packr_core::HostImports,
        _ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
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
    /// use theater::pack_bridge::{parse_pact, InterfaceImpl, TypeHash};
    ///
    /// // a handler owns its interface: keep the .pact in the handler crate
    /// const MY_PACT: &str = include_str!("../my-interface.pact");
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
    /// use theater::pack_bridge::{parse_pact, InterfaceImpl};
    ///
    /// // a handler owns its interface: keep the .pact in the handler crate
    /// const MY_PACT: &str = include_str!("../my-interface.pact");
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
