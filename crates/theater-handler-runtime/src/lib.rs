//! Theater Runtime Handler
//!
//! The runtime CONTROL interface `theater:simple/runtime`: spawn, inspect, and
//! drive actors, plus system-wide control. Actor lifecycle is a runtime
//! primitive — there is no separate supervisor handler and no view scope; the
//! runtime is flat and every op names its target actor by id.
//!
//! - `spawn` / `spawn-and-wait` — create an actor (setup + init) (mutate)
//! - `list-actors` — every actor in the runtime (inspect)
//! - `get-actor-status` / `-state` / `-manifest` (id) — live single-actor reads (inspect)
//! - `stop-actor` / `kill-actor` (id) — lifecycle control (mutate)
//! - `shutdown-runtime` — shut the whole runtime down (mutate)
//! - `subscribe-to-spawns` / `unsubscribe-from-spawns` — observe the actor
//!   population: every actor spawned anywhere is delivered to this actor's
//!   `handle-actor-spawn` export (births only; a death rides that actor's own
//!   chain subscription via `lifecycle.monitor`). (inspect)
//!
//! Capability-gated by RuntimePermissions { inspect, mutate }.

use serde::{Deserialize, Serialize};
use theater::actor::handle::ActorHandle;
use theater::actor::runtime::ActorRuntimeError;
use theater::chain::ChainEvent;
use theater::config::permissions::RuntimePermissions;
use theater::events::lifecycle::{ActorLifecycleEvent, TerminationCause};
use theater::events::{decode_chain_event_payload, ChainEventPayload};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{default_init_state, TheaterCommand};
use theater::shutdown::ShutdownReceiver;
use theater::utils::{resolve_reference, resolve_reference_cached, ResourceCache};
use theater::ManifestConfig;
use theater::SpawnError;

use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, InterfaceImpl, TypeHash, Value, ValueType,
};

use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use theater::id::TheaterId;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

/// Configuration for the runtime CONTROL handler (theater:simple/runtime).
/// Runtime-wide control plane (spawn/inspect/drive any actor); capability-gated
/// by RuntimePermissions { inspect, mutate }. No fields today.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeHostConfig {}

/// Embedded runtime.pact file content
const RUNTIME_PACT: &str = include_str!("../runtime.pact");

fn runtime_interface() -> InterfaceImpl {
    let pact = parse_pact(RUNTIME_PACT).expect("embedded runtime.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// Interface error for `theater:simple/runtime` — mirrors the `runtime-error`
/// pact variant. A normal Rust enum used with `?` throughout the ops; the single
/// `From<RuntimeError> for Value` below is the only place a pact error value is
/// built. Tags match the declaration order in runtime.pact.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// `theater_tx.send` or a response `recv` failed — the runtime's command
    /// channel is closed, i.e. the runtime is shutting down.
    #[error("runtime unavailable (shutting down)")]
    RuntimeUnavailable,
    #[error("actor not found: {0}")]
    ActorNotFound(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("spawn failed: {0}")]
    SpawnFailed(SpawnFailure),
    /// Opaque runtime op error not yet structured. The LAST resort: the runtime
    /// failed an op with an error we can't yet classify because it crosses the
    /// command boundary as a string.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Why a spawn failed — the structured payload of `runtime-error.spawn-failed`.
/// Every distinguishable way a `spawn`/`spawn-and-wait` can fail gets its own
/// case so the calling actor can react to the cause instead of substring-
/// matching a string. `Display` detail is preserved as the payload.
#[derive(Debug, Error)]
pub enum SpawnFailure {
    /// The manifest string could not be decoded, loaded, or parsed.
    #[error("bad manifest: {0}")]
    BadManifest(String),
    /// The actor's wasm bytes could not be fetched/loaded.
    #[error("wasm fetch failed: {0}")]
    WasmFetch(String),
    /// Building the actor's handler registry from its manifest failed.
    #[error("handler registry build failed: {0}")]
    HandlerRegistry(String),
    /// The wasm module failed to instantiate (bad binary, unresolved host
    /// import, PIC/packr-version skew).
    #[error("wasm invalid: {0}")]
    WasmInvalid(String),
    /// An imported interface's hash did not match the host's implementation.
    #[error("interface mismatch: {0}")]
    InterfaceMismatch(String),
    /// No handler provides an interface the actor imports (missing grant?).
    #[error("missing interface: {0}")]
    MissingInterface(String),
    /// The actor exports no `__pack_types` metadata — not a valid Pack actor.
    #[error("missing interface metadata: {0}")]
    MissingMetadata(String),
    /// The actor's `init` export returned an error or trapped.
    #[error("init failed: {0}")]
    InitFailed(String),
    /// (spawn-and-wait) the child actor errored while we waited for it.
    #[error("child failed: {0}")]
    ChildFailed(String),
    /// (spawn-and-wait) the child was stopped by something else while waiting.
    #[error("child stopped externally: {0}")]
    ChildStopped(String),
    /// (spawn-and-wait) the child did not complete within the timeout.
    #[error("timeout: {0}")]
    Timeout(String),
    /// A spawn-time host-internal failure the actor can't act on. Detail preserved.
    #[error("internal: {0}")]
    Internal(String),
}

impl From<SpawnFailure> for Value {
    fn from(e: SpawnFailure) -> Value {
        let (tag, case, m) = match e {
            SpawnFailure::BadManifest(m) => (0, "bad-manifest", m),
            SpawnFailure::WasmFetch(m) => (1, "wasm-fetch", m),
            SpawnFailure::HandlerRegistry(m) => (2, "handler-registry", m),
            SpawnFailure::WasmInvalid(m) => (3, "wasm-invalid", m),
            SpawnFailure::InterfaceMismatch(m) => (4, "interface-mismatch", m),
            SpawnFailure::MissingInterface(m) => (5, "missing-interface", m),
            SpawnFailure::MissingMetadata(m) => (6, "missing-metadata", m),
            SpawnFailure::InitFailed(m) => (7, "init-failed", m),
            SpawnFailure::ChildFailed(m) => (8, "child-failed", m),
            SpawnFailure::ChildStopped(m) => (9, "child-stopped", m),
            SpawnFailure::Timeout(m) => (10, "timeout", m),
            SpawnFailure::Internal(m) => (11, "internal", m),
        };
        Value::Variant {
            type_name: "spawn-failure".to_string(),
            case_name: case.to_string(),
            tag,
            payload: vec![Value::String(m)],
        }
    }
}

/// Map the runtime's structured spawn failure onto a `spawn-failure` cause. Each
/// distinguishable runtime cause becomes its own case; only genuinely
/// host-internal conditions fall back to `internal`, detail preserved.
impl From<SpawnError> for SpawnFailure {
    fn from(e: SpawnError) -> SpawnFailure {
        match e {
            SpawnError::HandlerRegistry(m) => SpawnFailure::HandlerRegistry(m),
            SpawnError::SetupChannelClosed => SpawnFailure::Internal(
                "actor setup task ended without reporting a result".to_string(),
            ),
            SpawnError::Init(err) => SpawnFailure::InitFailed(err.to_string()),
            SpawnError::Setup(setup) => {
                let detail = setup.to_string();
                match setup {
                    ActorRuntimeError::WasmInstantiationFailed { .. } => {
                        SpawnFailure::WasmInvalid(detail)
                    }
                    ActorRuntimeError::InterfaceHashMismatch { .. } => {
                        SpawnFailure::InterfaceMismatch(detail)
                    }
                    ActorRuntimeError::NoHandlerForInterface { .. } => {
                        SpawnFailure::MissingInterface(detail)
                    }
                    ActorRuntimeError::MissingInterfaceMetadata { .. } => {
                        SpawnFailure::MissingMetadata(detail)
                    }
                    ActorRuntimeError::FunctionTypeCacheFailed { .. }
                    | ActorRuntimeError::ActorInstanceNotFound { .. }
                    | ActorRuntimeError::ActorPhaseError { .. }
                    | ActorRuntimeError::ActorError(_)
                    | ActorRuntimeError::UnknownError(_) => SpawnFailure::Internal(detail),
                }
            }
        }
    }
}

/// The single translation from the Rust error to the `runtime-error` pact
/// variant. Tags match the declaration order in runtime.pact.
impl From<RuntimeError> for Value {
    fn from(e: RuntimeError) -> Value {
        let (tag, case, payload) = match e {
            RuntimeError::PermissionDenied(m) => (0, "permission-denied", vec![Value::String(m)]),
            RuntimeError::RuntimeUnavailable => (1, "runtime-unavailable", vec![]),
            RuntimeError::ActorNotFound(m) => (2, "actor-not-found", vec![Value::String(m)]),
            RuntimeError::InvalidArgument(m) => (3, "invalid-argument", vec![Value::String(m)]),
            RuntimeError::SpawnFailed(sf) => (4, "spawn-failed", vec![Value::from(sf)]),
            RuntimeError::Internal(m) => (5, "internal", vec![Value::String(m)]),
        };
        Value::Variant {
            type_name: "runtime-error".to_string(),
            case_name: case.to_string(),
            tag,
            payload,
        }
    }
}

/// Enforce the runtime capability. `mutate` = spawn/stop/kill/shutdown;
/// otherwise inspect (list/get/subscribe). Default-deny when the capability is
/// absent.
fn require(perms: &Option<RuntimePermissions>, mutate: bool) -> Result<(), RuntimeError> {
    let p = perms
        .as_ref()
        .ok_or_else(|| RuntimeError::PermissionDenied("runtime capability not granted".into()))?;
    let granted = if mutate { p.mutate } else { p.inspect };
    if granted {
        Ok(())
    } else {
        Err(RuntimeError::PermissionDenied(format!(
            "runtime '{}' capability not granted",
            if mutate { "mutate" } else { "inspect" }
        )))
    }
}

/// Parse a wire `actor-id` (a string) into a TheaterId.
fn parse_actor_id(id: &str) -> Result<TheaterId, RuntimeError> {
    id.parse()
        .map_err(|e| RuntimeError::InvalidArgument(format!("invalid actor id '{}': {}", id, e)))
}

/// Fetch the runtime's actor list: (id, name, parent-id).
async fn get_actors(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
) -> Result<Vec<(TheaterId, String, Option<TheaterId>)>, RuntimeError> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActors { response_tx: tx })
        .map_err(|_| RuntimeError::RuntimeUnavailable)?;
    match rx.await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(RuntimeError::Internal(e.to_string())),
        Err(_) => Err(RuntimeError::RuntimeUnavailable),
    }
}

/// Err with `actor-not-found` unless `target` is a live actor. The runtime is
/// flat (no view), so an absent target is honestly not-found.
async fn ensure_exists(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    target: TheaterId,
) -> Result<(), RuntimeError> {
    if get_actors(theater_tx)
        .await?
        .iter()
        .any(|(id, _, _)| *id == target)
    {
        Ok(())
    } else {
        Err(RuntimeError::ActorNotFound(target.to_string()))
    }
}

type SpawnEvent = (TheaterId, String, Option<TheaterId>);

/// The RuntimeHandler exposes the runtime CONTROL interface to a granted actor.
///
/// Per-actor instantiation goes through [`Self::fresh`] (via `create_instance`)
/// so each actor gets its own spawn-event channel.
#[derive(Clone)]
pub struct RuntimeHandler {
    event_tx: mpsc::Sender<SpawnEvent>,
    event_rx: Arc<Mutex<Option<mpsc::Receiver<SpawnEvent>>>>,
    /// Optional shared URL-bytes cache. When present, spawns whose manifest sets
    /// `static_package = true` fetch the wasm through this cache instead of
    /// re-resolving every time.
    resource_cache: Option<Arc<ResourceCache>>,
    permissions: Option<RuntimePermissions>,
}

impl RuntimeHandler {
    pub fn new(_config: RuntimeHostConfig, permissions: Option<RuntimePermissions>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            resource_cache: None,
            permissions,
        }
    }

    /// Wire in a shared `ResourceCache` so spawns whose manifest has
    /// `static_package = true` skip the wasm-bytes fetch on repeat calls.
    pub fn with_resource_cache(mut self, cache: Arc<ResourceCache>) -> Self {
        self.resource_cache = Some(cache);
        self
    }

    fn fresh(&self) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            resource_cache: self.resource_cache.clone(),
            permissions: self.permissions.clone(),
        }
    }
}

impl Handler for RuntimeHandler {
    fn create_instance(
        &self,
        _config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(self.fresh())
    }

    fn set_permissions(
        &mut self,
        permissions: Option<&theater::config::permissions::HandlerPermission>,
    ) {
        // Bake in this actor's granted runtime capability (the gate reads
        // self.permissions). `None` -> default-deny.
        self.permissions = permissions.and_then(|p| p.runtime.clone());
    }

    fn name(&self) -> &str {
        "runtime"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(
            self.interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect(),
        )
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/runtime-handlers".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![runtime_interface()]
    }

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up runtime (control) host functions (Pack)");
        if ctx.is_satisfied("theater:simple/runtime") {
            info!("theater:simple/runtime already satisfied by another handler, skipping");
            return Ok(());
        }

        let event_tx = self.event_tx.clone();
        let permissions = self.permissions.clone();
        let resource_cache = self.resource_cache.clone();
        let id = ctx
            .actor_id
            .ok_or_else(|| anyhow::anyhow!("actor_id not set in HandlerContext"))?;
        let theater_tx = ctx
            .theater_tx
            .clone()
            .ok_or_else(|| anyhow::anyhow!("theater_tx not set in HandlerContext"))?;

        // spawn: func(manifest, init-state, wasm-bytes) -> result<string, runtime-error>  (mutate)
        imports.define(
            "theater:simple/runtime",
            "spawn",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let resource_cache = resource_cache.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let resource_cache = resource_cache.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, true)?;
                        let (manifest_path, init_state_override, provided_wasm_bytes) = match input
                        {
                            Value::Tuple(mut args) if args.len() == 3 => {
                                let wasm_bytes = parse_optional_bytes(&args[2]);
                                let init_state_override = match args.remove(1) {
                                    Value::Option { value: None, .. } => None,
                                    Value::Option {
                                        value: Some(inner), ..
                                    } => Some(*inner),
                                    _ => {
                                        return Err(Value::from(RuntimeError::InvalidArgument(
                                            "init-state must be option<value>".to_string(),
                                        )))
                                    }
                                };
                                let manifest = match args.remove(0) {
                                    Value::String(s) => s,
                                    _ => {
                                        return Err(Value::from(RuntimeError::InvalidArgument(
                                            "invalid manifest argument".to_string(),
                                        )))
                                    }
                                };
                                (manifest, init_state_override, wasm_bytes)
                            }
                            _ => {
                                return Err(Value::from(RuntimeError::InvalidArgument(
                                    "expected (manifest, option<value>, option<list<u8>>)"
                                        .to_string(),
                                )))
                            }
                        };

                        let manifest = load_manifest(&manifest_path).await?;
                        let wasm_bytes =
                            resolve_wasm(&manifest, provided_wasm_bytes, resource_cache.as_deref())
                                .await?;

                        let name = Some(manifest.name.clone());
                        let init_state = resolve_init_state(init_state_override, &manifest);
                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::SpawnActor {
                            wasm_bytes,
                            name,
                            manifest: Some(manifest),
                            init_state,
                            response_tx,
                            subscription_tx: None,
                            // This actor is spawned by the calling actor.
                            parent_id: Some(id),
                        };
                        if theater_tx.send(cmd).is_err() {
                            return Err(Value::from(RuntimeError::RuntimeUnavailable));
                        }
                        match response_rx.await {
                            Ok(Ok(actor_id)) => Ok(Value::String(actor_id.to_string())),
                            Ok(Err(e)) => Err(Value::from(RuntimeError::SpawnFailed(
                                SpawnFailure::from(e),
                            ))),
                            Err(_) => Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        }
                    }
                }
            }),
        );

        // spawn-and-wait: func(manifest, init-state, wasm-bytes, timeout-ms) -> result<option<list<u8>>, runtime-error>  (mutate)
        imports.define(
            "theater:simple/runtime",
            "spawn-and-wait",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let resource_cache = resource_cache.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let resource_cache = resource_cache.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, true)?;
                        let (manifest_path, init_state_override, provided_wasm_bytes, timeout_ms) = match input {
                            Value::Tuple(mut args) if args.len() == 4 => {
                                let timeout_ms = parse_optional_u64(&args[3]);
                                let wasm_bytes = parse_optional_bytes(&args[2]);
                                let init_state_override = match args.remove(1) {
                                    Value::Option { value: None, .. } => None,
                                    Value::Option { value: Some(inner), .. } => Some(*inner),
                                    _ => return Err(Value::from(RuntimeError::InvalidArgument("init-state must be option<value>".to_string()))),
                                };
                                let manifest = match args.remove(0) {
                                    Value::String(s) => s,
                                    _ => return Err(Value::from(RuntimeError::InvalidArgument("invalid manifest argument".to_string()))),
                                };
                                (manifest, init_state_override, wasm_bytes, timeout_ms)
                            }
                            _ => return Err(Value::from(RuntimeError::InvalidArgument("expected (manifest, option<value>, option<list<u8>>, option<u64>)".to_string()))),
                        };

                        let manifest = load_manifest(&manifest_path).await?;
                        let wasm_bytes = resolve_wasm(&manifest, provided_wasm_bytes, resource_cache.as_deref()).await?;

                        // Subscribe to the child's chain AT SPAWN (subscription_tx
                        // registers before init, so the terminal event can't be
                        // missed).
                        let (ev_tx, mut ev_rx) = mpsc::channel::<(TheaterId, ChainEvent)>(64);
                        let name = Some(manifest.name.clone());
                        let init_state = resolve_init_state(init_state_override, &manifest);
                        let (response_tx, response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::SpawnActor {
                            wasm_bytes,
                            name,
                            manifest: Some(manifest),
                            init_state,
                            response_tx,
                            subscription_tx: Some(ev_tx),
                            parent_id: Some(id),
                        };
                        if theater_tx.send(cmd).is_err() {
                            return Err(Value::from(RuntimeError::RuntimeUnavailable));
                        }

                        let actor_id = match response_rx.await {
                            Ok(Ok(id)) => id,
                            Ok(Err(e)) => return Err(Value::from(RuntimeError::SpawnFailed(SpawnFailure::from(e)))),
                            Err(_) => return Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        };

                        // Drain the child's chain until its terminal event, then
                        // map the cause to spawn-and-wait's result.
                        let await_terminal = async {
                            while let Some((_, event)) = ev_rx.recv().await {
                                match decode_chain_event_payload(&event.data) {
                                    Some(ChainEventPayload::Lifecycle(
                                        ActorLifecycleEvent::Terminated { cause },
                                    )) => return Some(cause),
                                    _ => continue,
                                }
                            }
                            None
                        };
                        let wait_result = if let Some(ms) = timeout_ms {
                            tokio::time::timeout(Duration::from_millis(ms), await_terminal).await
                        } else {
                            Ok(await_terminal.await)
                        };

                        match wait_result {
                            Ok(Some(TerminationCause::Completed { final_state })) => {
                                Ok(option_bytes_to_value(final_state))
                            }
                            Ok(Some(TerminationCause::Failed { error })) => {
                                Err(Value::from(RuntimeError::SpawnFailed(SpawnFailure::ChildFailed(format!("child actor {} failed: {}", actor_id, error)))))
                            }
                            Ok(Some(cause)) => {
                                Err(Value::from(RuntimeError::SpawnFailed(SpawnFailure::ChildStopped(format!("child actor {} was stopped ({:?})", actor_id, cause)))))
                            }
                            Ok(None) => {
                                Err(Value::from(RuntimeError::Internal(format!("child actor {} chain closed before terminating", actor_id))))
                            }
                            Err(_) => {
                                let (stop_tx, _) = oneshot::channel();
                                let _ = theater_tx.send(TheaterCommand::StopActor {
                                    actor_id,
                                    response_tx: stop_tx,
                                });
                                Err(Value::from(RuntimeError::SpawnFailed(SpawnFailure::Timeout(format!("timeout waiting for child actor {} to complete", actor_id)))))
                            }
                        }
                    }
                }
            }),
        );

        // list-actors: func() -> result<list<actor-info>, runtime-error>  (inspect)
        imports.define(
            "theater:simple/runtime",
            "list-actors",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |_input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, false)?;
                        let actors = get_actors(&theater_tx).await?;
                        let items: Vec<Value> = actors
                            .iter()
                            .map(|(id, name, parent)| Value::Record {
                                type_name: "actor-info".to_string(),
                                fields: vec![
                                    ("id".to_string(), Value::String(id.to_string())),
                                    ("name".to_string(), Value::String(name.clone())),
                                    (
                                        "parent-id".to_string(),
                                        Value::Option {
                                            inner_type: ValueType::String,
                                            value: parent
                                                .map(|p| Box::new(Value::String(p.to_string()))),
                                        },
                                    ),
                                ],
                            })
                            .collect();
                        Ok::<Value, Value>(Value::List {
                            elem_type: ValueType::Record("actor-info".to_string()),
                            items,
                        })
                    }
                }
            }),
        );

        // get-actor-status: func(id) -> result<string, runtime-error>  (inspect)
        imports.define(
            "theater:simple/runtime",
            "get-actor-status",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, false)?;
                        let target = parse_target(input)?;
                        ensure_exists(&theater_tx, target).await?;
                        let (rtx, rrx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorStatus {
                                actor_id: target,
                                response_tx: rtx,
                            })
                            .map_err(|_| RuntimeError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(status)) => Ok(Value::String(format!("{:?}", status))),
                            Ok(Err(e)) => Err(Value::from(RuntimeError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        }
                    }
                }
            }),
        );

        // get-actor-state: func(id) -> result<option<list<u8>>, runtime-error>  (inspect)
        imports.define(
            "theater:simple/runtime",
            "get-actor-state",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, false)?;
                        let target = parse_target(input)?;
                        ensure_exists(&theater_tx, target).await?;
                        let (rtx, rrx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorState {
                                actor_id: target,
                                response_tx: rtx,
                            })
                            .map_err(|_| RuntimeError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(state)) => Ok(state),
                            Ok(Err(e)) => Err(Value::from(RuntimeError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        }
                    }
                }
            }),
        );

        // get-actor-manifest: func(id) -> result<string, runtime-error>  (inspect)
        imports.define(
            "theater:simple/runtime",
            "get-actor-manifest",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, false)?;
                        let target = parse_target(input)?;
                        ensure_exists(&theater_tx, target).await?;
                        let (rtx, rrx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorManifest {
                                actor_id: target,
                                response_tx: rtx,
                            })
                            .map_err(|_| RuntimeError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(m)) => {
                                serde_json::to_string(&m).map(Value::String).map_err(|e| {
                                    RuntimeError::Internal(format!("serialize manifest: {}", e))
                                        .into()
                                })
                            }
                            Ok(Err(e)) => Err(Value::from(RuntimeError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        }
                    }
                }
            }),
        );

        // stop-actor: func(id) -> result<_, runtime-error>  (mutate, graceful)
        imports.define(
            "theater:simple/runtime",
            "stop-actor",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, true)?;
                        let target = parse_target(input)?;
                        ensure_exists(&theater_tx, target).await?;
                        let (rtx, rrx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::StopActor {
                                actor_id: target,
                                response_tx: rtx,
                            })
                            .map_err(|_| RuntimeError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::from(RuntimeError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        }
                    }
                }
            }),
        );

        // kill-actor: func(id) -> result<_, runtime-error>  (mutate, force)
        imports.define(
            "theater:simple/runtime",
            "kill-actor",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, true)?;
                        let target = parse_target(input)?;
                        ensure_exists(&theater_tx, target).await?;
                        let (rtx, rrx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::TerminateActor {
                                actor_id: target,
                                response_tx: rtx,
                            })
                            .map_err(|_| RuntimeError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::from(RuntimeError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(RuntimeError::RuntimeUnavailable)),
                        }
                    }
                }
            }),
        );

        // shutdown-runtime: func() -> result<_, runtime-error>  (mutate)
        imports.define(
            "theater:simple/runtime",
            "shutdown-runtime",
            pact_result_host_fn({
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |_input: Value| {
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, true)?;
                        if theater_tx.send(TheaterCommand::ShutdownRuntime).is_err() {
                            return Err(Value::from(RuntimeError::RuntimeUnavailable));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            }),
        );

        // subscribe-to-spawns: func() -> result<_, runtime-error>  (inspect)
        imports.define(
            "theater:simple/runtime",
            "subscribe-to-spawns",
            pact_result_host_fn({
                let event_tx = event_tx.clone();
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |_input: Value| {
                    let event_tx = event_tx.clone();
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, false)?;
                        if theater_tx
                            .send(TheaterCommand::SubscribeToSpawns { event_tx })
                            .is_err()
                        {
                            return Err(Value::from(RuntimeError::RuntimeUnavailable));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            }),
        );

        // unsubscribe-from-spawns: func() -> result<_, runtime-error>  (inspect)
        imports.define(
            "theater:simple/runtime",
            "unsubscribe-from-spawns",
            pact_result_host_fn({
                let event_tx = event_tx.clone();
                let permissions = permissions.clone();
                let theater_tx = theater_tx.clone();
                move |_input: Value| {
                    let event_tx = event_tx.clone();
                    let permissions = permissions.clone();
                    let theater_tx = theater_tx.clone();
                    async move {
                        require(&permissions, false)?;
                        if theater_tx
                            .send(TheaterCommand::UnsubscribeFromSpawns { event_tx })
                            .is_err()
                        {
                            return Err(Value::from(RuntimeError::RuntimeUnavailable));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            }),
        );

        ctx.mark_satisfied("theater:simple/runtime");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        actor_instance: SharedActorInstance,
        mut shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Runtime (control) handler setup");
        let event_rx_opt = self.event_rx.lock().unwrap().take();

        Box::pin(async move {
            let Some(mut event_rx) = event_rx_opt else {
                info!("Runtime handler has no receiver (cloned instance), not starting");
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            // Does the actor implement the spawn-notification export?
            let has_spawn = {
                let mut instance_guard = actor_instance.write().await;
                if let Some(instance) = instance_guard.as_mut() {
                    instance
                        .has_export("theater:simple/runtime-handlers", "handle-actor-spawn")
                        .await
                        .unwrap_or(false)
                } else {
                    false
                }
            };

            loop {
                tokio::select! {
                    Some((id, name, parent)) = event_rx.recv() => {
                        if has_spawn {
                            let params = Value::Tuple(vec![
                                Value::String(id.to_string()),
                                Value::String(name),
                                Value::Option {
                                    inner_type: ValueType::String,
                                    value: parent.map(|p| Box::new(Value::String(p.to_string()))),
                                },
                            ]);
                            if let Err(e) = actor_handle
                                .call_function(
                                    "theater:simple/runtime-handlers.handle-actor-spawn".to_string(),
                                    params,
                                )
                                .await
                            {
                                error!("handle-actor-spawn failed: {}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_receiver.receiver => {
                        debug!("Runtime handler shutdown");
                        break;
                    }
                }
            }
            Ok(())
        })
    }
}

/// Parse the single `id` string argument (an actor id) from a call.
fn parse_target(input: Value) -> Result<TheaterId, Value> {
    match input {
        Value::String(s) => parse_actor_id(&s).map_err(Value::from),
        _ => Err(Value::from(RuntimeError::InvalidArgument(
            "expected actor id string".to_string(),
        ))),
    }
}

/// Resolve + parse a manifest reference into a `ManifestConfig`, mapping failures
/// onto the `spawn-failure.bad-manifest` cause.
async fn load_manifest(manifest_path: &str) -> Result<ManifestConfig, Value> {
    let manifest_str = match resolve_reference(manifest_path).await {
        Ok(bytes) => String::from_utf8(bytes).map_err(|e| {
            Value::from(RuntimeError::SpawnFailed(SpawnFailure::BadManifest(
                format!("invalid manifest encoding: {}", e),
            )))
        })?,
        Err(e) => {
            return Err(Value::from(RuntimeError::SpawnFailed(
                SpawnFailure::BadManifest(format!("failed to load manifest: {}", e)),
            )))
        }
    };
    ManifestConfig::from_toml_str(&manifest_str).map_err(|e| {
        Value::from(RuntimeError::SpawnFailed(SpawnFailure::BadManifest(
            format!("failed to parse manifest: {}", e),
        )))
    })
}

/// Resolve the actor's wasm bytes: caller-provided, cached fetch (static_package
/// + a wired cache), or a plain fetch. Failures map to `spawn-failure.wasm-fetch`.
async fn resolve_wasm(
    manifest: &ManifestConfig,
    provided: Option<Vec<u8>>,
    cache: Option<&ResourceCache>,
) -> Result<Vec<u8>, Value> {
    if let Some(bytes) = provided {
        return Ok(bytes);
    }
    fn fetch_err(e: impl std::fmt::Display) -> Value {
        Value::from(RuntimeError::SpawnFailed(SpawnFailure::WasmFetch(format!(
            "failed to load WASM: {}",
            e
        ))))
    }
    match (manifest.static_package, cache) {
        (true, Some(cache)) => match resolve_reference_cached(&manifest.package, cache).await {
            Ok((arc, _hit)) => Ok((*arc).clone()),
            Err(e) => Err(fetch_err(e)),
        },
        _ => resolve_reference(&manifest.package)
            .await
            .map_err(fetch_err),
    }
}

/// Resolve init-state: explicit override wins; else fall back to
/// `manifest.initial_state`; else the conventional none sentinel.
fn resolve_init_state(override_state: Option<Value>, manifest: &ManifestConfig) -> Value {
    match override_state {
        Some(v) => v,
        None => match manifest.initial_state.as_ref() {
            Some(s) => Value::String(s.clone()),
            None => default_init_state(),
        },
    }
}

/// Convert Option<Vec<u8>> to a Pack Value matching option<list<u8>>
fn option_bytes_to_value(data: Option<Vec<u8>>) -> Value {
    match data {
        Some(bytes) => Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: Some(Box::new(Value::List {
                elem_type: ValueType::U8,
                items: bytes.into_iter().map(Value::U8).collect(),
            })),
        },
        None => Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: None,
        },
    }
}

/// Parse an optional byte list from a Pack Value
fn parse_optional_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Option {
            value: Some(inner), ..
        } => {
            if let Value::List { items, .. } = inner.as_ref() {
                Some(
                    items
                        .iter()
                        .filter_map(|v| if let Value::U8(b) = v { Some(*b) } else { None })
                        .collect(),
                )
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse an optional u64 from a Pack Value
fn parse_optional_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Option {
            value: Some(inner), ..
        } => match inner.as_ref() {
            Value::U64(n) => Some(*n),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_interface_hash_determinism() {
        let a = runtime_interface();
        let b = runtime_interface();
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a.name(), "theater:simple/runtime");
    }

    #[test]
    fn test_handler_name_and_exports() {
        let h = RuntimeHandler::new(RuntimeHostConfig {}, None);
        assert_eq!(h.name(), "runtime");
        assert_eq!(
            h.exports(),
            Some(vec!["theater:simple/runtime-handlers".to_string()])
        );
    }

    #[test]
    fn test_require_gate() {
        assert!(require(&None, false).is_err());
        assert!(require(&None, true).is_err());
        let ro = Some(RuntimePermissions {
            inspect: true,
            mutate: false,
        });
        assert!(require(&ro, false).is_ok());
        assert!(require(&ro, true).is_err());
        let rw = Some(RuntimePermissions {
            inspect: true,
            mutate: true,
        });
        assert!(require(&rw, false).is_ok());
        assert!(require(&rw, true).is_ok());
    }
}
