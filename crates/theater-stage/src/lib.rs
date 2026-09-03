//! # theater-stage
//!
//! The **composition root** for Theater. `theater` is a library — the runtime
//! mechanism — and knows about no concrete handlers. Something has to name the
//! actual capability set a node exposes and wire it together; that is this crate.
//! It's the stage the actors perform on.
//!
//! [`standard_handlers`] builds a [`HandlerRegistry`] with the standard battery of
//! handlers (self, lifecycle, store, supervisor, runtime, message-server, rpc,
//! tcp, terminal, timer, loop, podman, http-client). It is the *single* answer to
//! "what is a standard Theater node," reused by the CLI, the integration tests,
//! and any embedder — instead of each hand-rolling the same registration.
//!
//! Registration is **explicit** (not link-time `inventory`/`typetag` magic): for a
//! capability runtime, which host capabilities are loaded must be a list you can
//! read here, not an emergent property of what got linked. A third party adds a
//! capability by registering their handler on top:
//!
//! ```ignore
//! let mut registry = theater_stage::standard_handlers(theater_tx, &Default::default());
//! registry.register(AcmeHandler::default());
//! ```

use std::sync::Arc;

use theater::config::actor_manifest::{
    HttpClientHandlerConfig, PodmanHandlerConfig, RuntimeHostConfig, SelfHostConfig,
    StoreHandlerConfig, SupervisorHostConfig, TcpHandlerConfig, TerminalHandlerConfig,
    TimerHandlerConfig,
};
use theater::handler::HandlerRegistry;
use theater::messages::TheaterCommand;
use theater::utils::ResourceCache;
use tokio::sync::mpsc::UnboundedSender;

use theater_handler_http_client::HttpClientHandler;
use theater_handler_lifecycle::LifecycleHandler;
use theater_handler_loop::LoopHandler;
use theater_handler_message_server::{MessageRouter, MessageServerHandler};
use theater_handler_podman::PodmanHandler;
use theater_handler_rpc::RpcHandler;
use theater_handler_runtime::RuntimeHandler;
use theater_handler_self::SelfHandler;
use theater_handler_store::StoreHandler;
use theater_handler_supervisor::SupervisorHandler;
use theater_handler_tcp::TcpHandler;
use theater_handler_terminal::TerminalHandler;
use theater_handler_timer::TimerHandler;

/// Options for assembling the standard handler set.
pub struct StandardHandlers {
    /// Stream each actor's `log` host calls to the process's tracing output.
    pub show_actor_logs: bool,
    /// Shared fetch cache for `static_package` child spawns. Handed to the
    /// supervisor handler so repeat spawns of the same wasm hit the cache.
    pub resource_cache: Arc<ResourceCache>,
}

impl Default for StandardHandlers {
    fn default() -> Self {
        Self {
            show_actor_logs: false,
            resource_cache: Arc::new(ResourceCache::new()),
        }
    }
}

/// Build a [`HandlerRegistry`] with the standard battery of Theater handlers.
///
/// This is the composition root: the one place that declares which host
/// capabilities a standard node provides. Handlers are registered as templates;
/// the runtime clones each per actor and threads the manifest's config +
/// effective permissions in at spawn time.
pub fn standard_handlers(
    theater_tx: UnboundedSender<TheaterCommand>,
    opts: &StandardHandlers,
) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();

    // Self — the per-actor handle: log, self, shutdown.
    registry.register(
        SelfHandler::new(SelfHostConfig {}, theater_tx.clone(), None)
            .with_show_logs(opts.show_actor_logs),
    );

    // Lifecycle — actor-facing link / monitor over the subscription substrate.
    registry.register(LifecycleHandler::new(theater_tx.clone()));

    // Store — content-addressed storage.
    registry.register(StoreHandler::new(StoreHandlerConfig::default(), None));

    // Supervisor — spawn/manage child actors; wired to the shared fetch cache.
    registry.register(
        SupervisorHandler::new(SupervisorHostConfig {}, None)
            .with_resource_cache(opts.resource_cache.clone()),
    );

    // Runtime (system) — shutdown-runtime + subscribe-to-spawns; capability-gated.
    registry.register(RuntimeHandler::new(RuntimeHostConfig {}, None));

    // Message-server — inter-actor messaging.
    registry.register(MessageServerHandler::new(None, MessageRouter::new()));

    // RPC — direct actor-to-actor function calls.
    registry.register(RpcHandler::new(theater_tx.clone()));

    // TCP — server/client sockets.
    registry.register(TcpHandler::new(TcpHandlerConfig {
        listen: None,
        max_connections: None,
        ..Default::default()
    }));

    // Terminal — stdin/stdout/stderr for interactive apps.
    registry.register(TerminalHandler::new(TerminalHandlerConfig::default()));

    // Timer — periodic tick callbacks.
    registry.register(TimerHandler::new(TimerHandlerConfig::default()));

    // Loop — cooperative looping with yield points.
    registry.register(LoopHandler::new());

    // Podman — container management via the podman CLI.
    registry.register(PodmanHandler::new(PodmanHandlerConfig::default()));

    // HTTP client — outbound HTTP(S), per-manifest allowed_hosts.
    registry.register(HttpClientHandler::new(HttpClientHandlerConfig::default()));

    registry
}
