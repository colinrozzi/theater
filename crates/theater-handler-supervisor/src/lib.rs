//! Theater Supervisor Handler
//!
//! Provides supervisor capabilities for spawning and managing child actors.

pub mod events;

use theater::actor::handle::ActorHandle;
use theater::actor::runtime::ActorRuntimeError;
use theater::actor::store::ActorStore;
use theater::chain::ChainEvent;
use theater::config::actor_manifest::SupervisorHostConfig;
use theater::config::permissions::{SupervisorPermissions, ViewScope};
use theater::events::lifecycle::{ActorLifecycleEvent, TerminationCause};
use theater::events::{decode_chain_event_payload, ChainEventPayload};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{default_init_state, TheaterCommand};
use theater::shutdown::ShutdownReceiver;
use theater::utils::{resolve_reference, resolve_reference_cached, ResourceCache};
use theater::ManifestConfig;
use theater::SpawnError;

// Pack integration
use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use theater::id::TheaterId;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Drops at scope exit and emits a `phase=... elapsed_ms=...` debug line.
/// One line per host fn invocation, on every return path including `?`
/// short-circuits. Complements the multi-step `phase = "supervisor.<step>"`
/// info! lines on the spawn pipeline; this guard is for host fns whose
/// cost is not interesting enough to merit a multi-phase breakdown.
struct PhaseLog {
    name: &'static str,
    start: Instant,
}

impl PhaseLog {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for PhaseLog {
    fn drop(&mut self) {
        debug!(
            phase = self.name,
            elapsed_ms = self.start.elapsed().as_millis() as u64,
            "supervisor phase complete",
        );
    }
}

/// Embedded supervisor.pact file content
const SUPERVISOR_PACT: &str = include_str!("../supervisor.pact");

/// Declare the theater:simple/supervisor interface from the pact file.
///
/// Actor-management, scoped to the caller's VIEW (its subtree, or `all` for a
/// control actor — see [`SupervisorPermissions`]). Every op is evaluated against
/// that view; a target outside it is rejected with `out-of-view`. Ops:
/// - spawn / spawn-and-wait — create a child of the caller (setup + init)
/// - list-actors -> list<actor-info> — every actor in view
/// - get-actor-status / -state / -manifest / -metrics (id)
/// - stop-actor (graceful) / kill-actor (force) (id)
/// - subscribe-to-actor / unsubscribe-from-actor (id) — chain events to the
///   caller's `handle-actor-event` export
///
/// `spawn` / `spawn-and-wait` are setup + init: the runtime calls the child's
/// `theater:simple/actor.init` export before returning the child id. `init-state`:
///   - `none` falls back to the child's `manifest.initial_state`.
///   - `some(v)` overrides unconditionally (even `some(none)` = "explicitly no state").
///
/// Recovery is just `spawn` of a fresh actor; there is no `resume` here — reading
/// or replaying persisted history is the recorder's domain, not this interface's.
///
/// Note: chain-event is approximated as list<u8> for interface hashing.
fn supervisor_interface() -> InterfaceImpl {
    let pact = parse_pact(SUPERVISOR_PACT).expect("embedded supervisor.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// Interface error for `theater:simple/supervisor` — mirrors the
/// `supervisor-error` pact variant. A normal Rust enum used with `?` throughout
/// the ops; the single `From<SupervisorError> for Value` below is the only
/// place a pact error value is built.
#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("actor not found: {0}")]
    ActorNotFound(String),
    #[error("out of view: {0}")]
    OutOfView(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("spawn failed: {0}")]
    SpawnFailed(SpawnFailure),
    /// `theater_tx.send` or a response `recv` failed — the runtime's command
    /// channel is closed, i.e. the runtime is shutting down. A distinct,
    /// nameable condition, not an open-ended internal.
    #[error("runtime unavailable (shutting down)")]
    RuntimeUnavailable,
    /// Opaque runtime op error not yet structured. This is deliberately the
    /// LAST resort: the runtime failed an op (e.g. a `spawn`/get race) with an
    /// error we can't yet classify because it crosses the command boundary as a
    /// string. See the `structured-runtime-errors` follow-up project — surfacing
    /// `TheaterRuntimeError` through the boundary is what replaces this.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Why a spawn failed — the structured payload of `supervisor-error.spawn-failed`.
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
    /// A spawn-time host-internal failure (function-type cache, phase invariant,
    /// unknown) the actor can't act on. Detail preserved.
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

/// The single translation from the Rust error to the `supervisor-error` pact
/// variant. Tags match the declaration order in supervisor.pact.
impl From<SupervisorError> for Value {
    fn from(e: SupervisorError) -> Value {
        let (tag, case, payload) = match e {
            SupervisorError::ActorNotFound(m) => (0, "actor-not-found", vec![Value::String(m)]),
            SupervisorError::OutOfView(m) => (1, "out-of-view", vec![Value::String(m)]),
            SupervisorError::PermissionDenied(m) => {
                (2, "permission-denied", vec![Value::String(m)])
            }
            SupervisorError::InvalidArgument(m) => (3, "invalid-argument", vec![Value::String(m)]),
            SupervisorError::SpawnFailed(sf) => (4, "spawn-failed", vec![Value::from(sf)]),
            SupervisorError::RuntimeUnavailable => (5, "runtime-unavailable", vec![]),
            SupervisorError::Internal(m) => (6, "internal", vec![Value::String(m)]),
        };
        Value::Variant {
            type_name: "supervisor-error".to_string(),
            case_name: case.to_string(),
            tag,
            payload,
        }
    }
}

/// Map the runtime's structured spawn failure onto the supervisor-error the
/// calling actor sees. This is the seam that pushes handling into the actor
/// layer: each distinguishable runtime cause becomes its own case (the actor
/// decides what to do), and only genuinely host-internal conditions fall back to
/// `internal`. The `Display` detail is preserved as the case payload.
/// Parse a wire `actor-id` (a string, decoded by packr from the typed param)
/// into a TheaterId.
fn parse_actor_id(id: &str) -> Result<TheaterId, SupervisorError> {
    id.parse()
        .map_err(|e| SupervisorError::InvalidArgument(format!("invalid actor id '{}': {}", id, e)))
}

/// Fetch the runtime's actor list: (id, name, parent-id).
async fn get_actors(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
) -> Result<Vec<(TheaterId, String, Option<TheaterId>)>, SupervisorError> {
    let (tx, rx) = oneshot::channel();
    theater_tx
        .send(TheaterCommand::GetActors { response_tx: tx })
        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
    match rx.await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(SupervisorError::Internal(e.to_string())),
        Err(_) => Err(SupervisorError::RuntimeUnavailable),
    }
}

/// The capability half of the gate: the caller must hold the capability the op
/// needs — `inspect` for reads/subscribe, `mutate` for lifecycle. Returns the
/// granted permission (so callers can read its `scope`). Default-deny when the
/// capability is absent. Used directly by no-target ops (list-actors, spawn) and
/// reused by [`authorize`] for single-target ops.
fn require_capability(
    perms: &Option<SupervisorPermissions>,
    mutate: bool,
) -> Result<&SupervisorPermissions, SupervisorError> {
    let p = perms.as_ref().ok_or_else(|| {
        SupervisorError::PermissionDenied("supervisor capability not granted".into())
    })?;
    let granted = if mutate { p.mutate } else { p.inspect };
    if granted {
        Ok(p)
    } else {
        Err(SupervisorError::PermissionDenied(format!(
            "supervisor '{}' capability not granted",
            if mutate { "mutate" } else { "inspect" }
        )))
    }
}

/// The full gate for a single-target op: capability (via [`require_capability`])
/// AND view — the target must be in the caller's view. Under direct-children
/// view-scope the caller's `children` set *is* its subtree; the runtime holds no
/// lineage, so the supervisor answers view entirely from what it spawned.
async fn authorize(
    theater_tx: &mpsc::UnboundedSender<TheaterCommand>,
    perms: &Option<SupervisorPermissions>,
    target: TheaterId,
    mutate: bool,
    children: &Arc<Mutex<HashSet<TheaterId>>>,
) -> Result<(), SupervisorError> {
    let scope = require_capability(perms, mutate)?.scope;
    match scope {
        ViewScope::All => {
            // Full visibility: an absent target is honestly actor-not-found.
            if get_actors(theater_tx)
                .await?
                .iter()
                .any(|(id, _, _)| *id == target)
            {
                Ok(())
            } else {
                Err(SupervisorError::ActorNotFound(target.to_string()))
            }
        }
        ViewScope::Subtree => {
            // Direct-children view: the caller supervises exactly the actors it
            // spawned; deeper actors belong to child supervisors. Uniform
            // out-of-view (do NOT distinguish "doesn't exist" from "not yours")
            // so the view boundary never leaks the existence of other actors.
            let in_view = children.lock().unwrap().contains(&target);
            if in_view {
                Ok(())
            } else {
                Err(SupervisorError::OutOfView(target.to_string()))
            }
        }
    }
}

/// The SupervisorHandler provides child actor management capabilities.
///
/// This handler enables actors to:
/// - Spawn new child actors
/// - Resume child actors from saved state
/// - List, restart, and stop children
/// - Get child state
/// - Receive notifications when children error, exit, or are stopped
/// - Opt in to per-child chain-event delivery via `subscribe-to-actor`
///   (default is opt-out: a freshly-spawned child sends no chain
///   events to its parent until the parent subscribes)
/// - Clean up children on shutdown
///
/// NOTE: the derived `Clone` shares `event_tx`/`event_rx`/`children` via
/// `Arc`/`Sender`. That sharing is deliberately NOT used for per-actor
/// instantiation — `create_instance` calls [`Self::fresh`] so each
/// supervisor-capable actor gets its own event channel and children set and thus
/// its own monitor loop. Do not swap that back to `self.clone()`, or every
/// non-root actor's supervisor loop goes dark and only the root can supervise.
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct SupervisorHandler {
    /// Channel for receiving all ChainEvents from watched actors. Every child is
    /// auto-subscribed at spawn, and each event is tagged with its source
    /// TheaterId before landing here, so dispatch can attribute it to the right
    /// actor even though N sources share this single receiver. A child's terminal
    /// event drives `handle-lifecycle-event` + tracking cleanup; non-terminal
    /// activity drives `handle-actor-event`.
    event_tx: tokio::sync::mpsc::Sender<(TheaterId, ChainEvent)>,
    event_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<(TheaterId, ChainEvent)>>>>,
    /// Set of currently active child actor IDs
    children: Arc<Mutex<HashSet<TheaterId>>>,
    /// Theater command sender for stopping children on shutdown
    theater_tx: Arc<Mutex<Option<mpsc::UnboundedSender<TheaterCommand>>>>,
    /// Optional shared URL-bytes cache. When present, spawns of children
    /// whose manifest sets `static_package = true` fetch the wasm
    /// through this cache instead of re-resolving every time.
    /// Constructed once at server bootstrap and shared across every
    /// supervisor-capable actor; see [`Self::with_resource_cache`].
    resource_cache: Option<Arc<ResourceCache>>,
    #[allow(dead_code)]
    permissions: Option<SupervisorPermissions>,
}

impl SupervisorHandler {
    /// Create a new SupervisorHandler
    ///
    /// # Arguments
    /// * `config` - Configuration for the supervisor handler
    /// * `permissions` - Optional permission restrictions
    ///
    /// # Returns
    /// The SupervisorHandler (receiver is stored internally)
    pub fn new(_config: SupervisorHostConfig, permissions: Option<SupervisorPermissions>) -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            children: Arc::new(Mutex::new(HashSet::new())),
            theater_tx: Arc::new(Mutex::new(None)),
            resource_cache: None,
            permissions,
        }
    }

    /// Wire in a shared `ResourceCache` so spawns of children whose
    /// manifest has `static_package = true` skip the wasm-bytes fetch on
    /// repeat calls. Without this the handler still works — the
    /// `static_package` flag is just ignored and every spawn refetches.
    pub fn with_resource_cache(mut self, cache: Arc<ResourceCache>) -> Self {
        self.resource_cache = Some(cache);
        self
    }

    /// Get a clone of the child-event sender.
    pub fn get_sender(&self) -> tokio::sync::mpsc::Sender<(TheaterId, ChainEvent)> {
        self.event_tx.clone()
    }

    /// Get a clone of the aggregated event subscription sender.
    ///
    /// Events on this channel are tagged with the source child's
    /// TheaterId — the handler's per-child forwarders attach the id
    /// before pushing. External callers wiring a custom event flow
    /// must do the same tagging.
    pub fn get_event_sender(&self) -> tokio::sync::mpsc::Sender<(TheaterId, ChainEvent)> {
        self.event_tx.clone()
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![supervisor_interface()]
    }

    /// Dispatch one chain event from a watched actor.
    ///
    /// A source's **terminal** lifecycle event is the death signal: drop it from
    /// child tracking and deliver the single `handle-lifecycle-event` callback.
    /// Every other event is non-terminal activity, delivered to
    /// `handle-actor-event` (for actors that export it). The terminal payload
    /// carries the cause + final state; the actor decodes `data` if it cares.
    /// Both callbacks pass the raw event `(id, event-type, data)`.
    async fn process_child_event(
        actor_handle: &ActorHandle,
        event_with_id: (TheaterId, ChainEvent),
        children: &Arc<Mutex<HashSet<TheaterId>>>,
        has_child_event: bool,
        has_lifecycle: bool,
    ) -> Result<()> {
        let (source_id, event) = event_with_id;
        debug!(
            "Supervisor received event from {}: type={}, data_len={}",
            source_id,
            event.event_type,
            event.data.len()
        );

        let is_terminal = matches!(
            decode_chain_event_payload(&event.data),
            Some(ChainEventPayload::Lifecycle(
                ActorLifecycleEvent::Terminated { .. }
            ))
        );

        let deliver = |name: &str| {
            let params = Value::Tuple(vec![
                Value::String(source_id.to_string()),
                Value::String(event.event_type.clone()),
                Value::List {
                    elem_type: ValueType::U8,
                    items: event.data.iter().map(|b| Value::U8(*b)).collect(),
                },
            ]);
            actor_handle.call_function(name.to_string(), params)
        };

        if is_terminal {
            // Source terminated: stop tracking it (if it was a child) and
            // deliver the one death callback.
            if children.lock().unwrap().remove(&source_id) {
                debug!("child {} terminated; dropped from tracking", source_id);
            }
            if has_lifecycle {
                deliver("theater:simple/supervisor-handlers.handle-lifecycle-event").await?;
            }
        } else if has_child_event {
            deliver("theater:simple/supervisor-handlers.handle-actor-event").await?;
        }

        Ok(())
    }
}

impl SupervisorHandler {
    /// Build a genuinely independent supervisor instance: fresh lifecycle
    /// and event channels, and a fresh (empty) children set. Only
    /// process-global config — the URL-bytes [`ResourceCache`] and the
    /// permissions — is carried forward from `self`.
    ///
    /// This is what per-actor instantiation must use. A plain `self.clone()`
    /// (the derived `Clone`) shares `event_rx`/`children` via `Arc`/`Sender`
    /// across *every* supervisor-capable actor, so only the first instance to run
    /// `setup` wins the `event_rx.take()` and runs the single monitor loop —
    /// collapsing any multi-level supervision tree to one root-only supervisor. A
    /// `fresh` instance instead gets its own `event_rx` (its own monitor loop
    /// starts) and its own `event_tx` (auto-subscribed onto *its* children, so
    /// their events route back to *its* callbacks), giving real hierarchical
    /// supervision.
    fn fresh(&self) -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            children: Arc::new(Mutex::new(HashSet::new())),
            theater_tx: Arc::new(Mutex::new(None)),
            resource_cache: self.resource_cache.clone(),
            permissions: self.permissions.clone(),
        }
    }
}

impl Handler for SupervisorHandler {
    fn create_instance(
        &self,
        _config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        // Each actor must get an INDEPENDENT supervisor — see `fresh`. A
        // shared clone would make only the root actor's monitor loop run.
        Box::new(self.fresh())
    }

    fn set_permissions(
        &mut self,
        permissions: Option<&theater::config::permissions::HandlerPermission>,
    ) {
        // Bake in this actor's granted supervisor capability (the gate reads
        // self.permissions). `None` -> default-deny.
        self.permissions = permissions.and_then(|p| p.supervisor.clone());
    }

    fn name(&self) -> &str {
        "supervisor"
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
        Some(vec!["theater:simple/supervisor-handlers".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<theater::pack_bridge::InterfaceImpl> {
        vec![supervisor_interface()]
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up supervisor host functions (Pack)");

        // Check if already satisfied
        if ctx.is_satisfied("theater:simple/supervisor") {
            info!("theater:simple/supervisor already satisfied by another handler, skipping");
            return Ok(());
        }

        let event_tx = self.event_tx.clone();
        let children = self.children.clone();
        let theater_tx_holder = self.theater_tx.clone();
        let resource_cache = self.resource_cache.clone();
        let permissions = self.permissions.clone();

        builder.interface("theater:simple/supervisor")?
            // spawn: func(manifest: string, wasm-bytes: option<list<u8>>) -> result<string, string>
            // Spawns a child actor. If wasm-bytes is provided, uses those bytes instead of loading from manifest.package.
            .func_async_result("spawn", {
                let event_tx = event_tx.clone();
                let children = children.clone();
                let theater_tx_holder = theater_tx_holder.clone();
                let resource_cache = resource_cache.clone();
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let event_tx = event_tx.clone();
                    let children = children.clone();
                    let theater_tx_holder = theater_tx_holder.clone();
                    let resource_cache = resource_cache.clone();
                    let permissions = permissions.clone();
                    async move {
                        // spawn creates a new actor parented to the caller; needs mutate.
                        require_capability(&permissions, true)?;
                        // Parse input: (string, option<value>, option<list<u8>>)
                        // init-state: None  → fall back to manifest.initial_state
                        //             Some(v) → use v verbatim (even if v is Value::Option::None)
                        let (manifest_path, init_state_override, provided_wasm_bytes) = match input {
                            Value::Tuple(mut args) if args.len() == 3 => {
                                let wasm_bytes = parse_optional_bytes(&args[2]);
                                let init_state_override = match args.remove(1) {
                                    Value::Option { value: None, .. } => None,
                                    Value::Option { value: Some(inner), .. } => Some(*inner),
                                    _ => return Err(Value::from(SupervisorError::InvalidArgument("init-state must be option<value>".to_string()))),
                                };
                                let manifest = match args.remove(0) {
                                    Value::String(s) => s,
                                    _ => return Err(Value::from(SupervisorError::InvalidArgument("invalid manifest argument".to_string()))),
                                };
                                (manifest, init_state_override, wasm_bytes)
                            }
                            _ => return Err(Value::from(SupervisorError::InvalidArgument("expected (manifest, option<value>, option<list<u8>>)".to_string()))),
                        };

                        let wasm_provided = provided_wasm_bytes.is_some();
                        if let Some(ref bytes) = provided_wasm_bytes {
                            debug!("spawn: manifest={}, wasm_bytes={} bytes", manifest_path, bytes.len());
                        } else {
                            debug!("spawn: manifest={}, wasm_bytes=None (will load from manifest.package)", manifest_path);
                        }

                        // spawn-bench: end-to-end + per-phase timing. Each
                        // phase emits an info! with elapsed_ms; the outer
                        // total is reported on the success path below.
                        let spawn_start = Instant::now();

                        // Load and parse manifest
                        let phase_start = Instant::now();
                        let manifest_str = match resolve_reference(&manifest_path).await {
                            Ok(bytes) => match String::from_utf8(bytes) {
                                Ok(s) => s,
                                Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::BadManifest(format!("invalid manifest encoding: {}", e))))),
                            },
                            Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::BadManifest(format!("failed to load manifest: {}", e))))),
                        };
                        info!(
                            phase = "supervisor.manifest_resolve",
                            manifest = %manifest_path,
                            bytes = manifest_str.len(),
                            elapsed_ms = phase_start.elapsed().as_millis() as u64,
                            "spawn phase complete",
                        );

                        let phase_start = Instant::now();
                        let manifest = match ManifestConfig::from_toml_str(&manifest_str) {
                            Ok(m) => m,
                            Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::BadManifest(format!("failed to parse manifest: {}", e))))),
                        };
                        info!(
                            phase = "supervisor.manifest_parse",
                            manifest = %manifest_path,
                            elapsed_ms = phase_start.elapsed().as_millis() as u64,
                            "spawn phase complete",
                        );

                        // Resolve wasm bytes.
                        //
                        // Three paths: caller-provided bytes (no fetch),
                        // cached fetch (when manifest opts in via
                        // static_package and the handler was wired with a
                        // ResourceCache), or plain uncached fetch.
                        let phase_start = Instant::now();
                        let mut cache_hit = false;
                        let mut used_cache = false;
                        let wasm_bytes = match provided_wasm_bytes {
                            Some(bytes) => bytes,
                            None => {
                                match (manifest.static_package, resource_cache.as_deref()) {
                                    (true, Some(cache)) => {
                                        used_cache = true;
                                        match resolve_reference_cached(&manifest.package, cache).await {
                                            Ok((arc, hit)) => {
                                                cache_hit = hit;
                                                // Caller expects owned bytes; the
                                                // cache returns Arc to deduplicate
                                                // RAM, but the spawn pipeline
                                                // currently takes Vec<u8> by value
                                                // into the runtime command. One
                                                // copy per spawn is the price of
                                                // not threading Arc through the
                                                // whole runtime; cheap relative to
                                                // compile.
                                                (*arc).clone()
                                            }
                                            Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::WasmFetch(format!("failed to load WASM: {}", e))))),
                                        }
                                    }
                                    _ => match resolve_reference(&manifest.package).await {
                                        Ok(bytes) => bytes,
                                        Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::WasmFetch(format!("failed to load WASM: {}", e))))),
                                    },
                                }
                            }
                        };
                        info!(
                            phase = "supervisor.wasm_resolve",
                            manifest = %manifest_path,
                            bytes = wasm_bytes.len(),
                            provided = wasm_provided,
                            used_cache,
                            cache_hit,
                            elapsed_ms = phase_start.elapsed().as_millis() as u64,
                            "spawn phase complete",
                        );

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();
                        let name = Some(manifest.name.clone());
                        let (response_tx, response_rx) = oneshot::channel();
                        // Resolve init-state: explicit override wins; else fall
                        // back to manifest.initial_state; else the conventional
                        // none sentinel. Matches the CLI's resolution (theater
                        // spawn does this same fallback in spawn.rs), so the
                        // wasm-facing and CLI-facing spawn paths agree on what
                        // an init-state-less spawn does.
                        let init_state = match init_state_override {
                            Some(v) => v,
                            None => match manifest.initial_state.as_ref() {
                                Some(s) => Value::String(s.clone()),
                                None => default_init_state(),
                            },
                        };
                        // The runtime notifies nobody: it holds no supervisor
                        // channel for this child (`supervisor_tx: None`). Instead
                        // the supervisor auto-monitors the child's chain right
                        // after spawn (below), so the child's terminal event drives
                        // `handle-lifecycle-event` + tracking cleanup — supervision
                        // lives entirely in the handler.
                        let cmd = TheaterCommand::SpawnActor {
                            wasm_bytes,
                            name,
                            manifest: Some(manifest),
                            init_state,
                            response_tx,
                            subscription_tx: None,
                            // This child is spawned by the calling actor.
                            parent_id: Some(store.id),
                        };

                        // runtime_setup_and_init covers: send to runtime command
                        // channel, runtime drains it, build_actor_resources runs,
                        // detached init fires response_tx. The latency here is
                        // the runtime command loop's serialized cost per spawn.
                        let phase_start = Instant::now();
                        if theater_tx.send(cmd).is_err() {
                            return Err(Value::from(SupervisorError::RuntimeUnavailable));
                        }

                        // Store theater_tx for shutdown (first spawn stores it)
                        {
                            let mut holder = theater_tx_holder.lock().unwrap();
                            if holder.is_none() {
                                *holder = Some(theater_tx);
                            }
                        }

                        match response_rx.await {
                            Ok(Ok(actor_id)) => {
                                let setup_elapsed = phase_start.elapsed().as_millis() as u64;
                                info!(
                                    phase = "supervisor.runtime_setup_and_init",
                                    manifest = %manifest_path,
                                    actor_id = %actor_id,
                                    elapsed_ms = setup_elapsed,
                                    "spawn phase complete",
                                );
                                info!(
                                    phase = "supervisor.spawn_total",
                                    manifest = %manifest_path,
                                    actor_id = %actor_id,
                                    elapsed_ms = spawn_start.elapsed().as_millis() as u64,
                                    "spawn total complete",
                                );
                                // Track the child
                                {
                                    let mut children_guard = children.lock().unwrap();
                                    children_guard.insert(actor_id);
                                    debug!("Tracking child {}, total children: {}", actor_id, children_guard.len());
                                }
                                // Auto-monitor: subscribe to the child's chain so
                                // its terminal event drives handle-lifecycle-event
                                // + tracking cleanup. Best-effort — a failure here
                                // just means no death notification for this child.
                                if ctx
                                    .data()
                                    .theater_tx
                                    .send(TheaterCommand::SubscribeToActor {
                                        actor_id,
                                        event_tx: event_tx.clone(),
                                    })
                                    .is_err()
                                {
                                    warn!("auto-monitor: failed to subscribe to child {}", actor_id);
                                }
                                Ok(Value::String(actor_id.to_string()))
                            }
                            Ok(Err(e)) => Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::from(e)))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // spawn-and-wait: func(manifest: string, init-state: option<value>, wasm-bytes: option<list<u8>>, timeout-ms: option<u64>) -> result<option<list<u8>>, string>
            // Spawns a child actor (setup + init) and waits for it to complete.
            // Returns the child's final result. If timeout-ms is provided,
            // returns an error if the child doesn't complete within that time.
            // Same init-state semantics as `spawn` — see that function's docs.
            .func_async_result("spawn-and-wait", {
                let resource_cache = resource_cache.clone();
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let resource_cache = resource_cache.clone();
                    let permissions = permissions.clone();
                    async move {
                        // spawn-and-wait creates a new actor parented to the caller; needs mutate.
                        require_capability(&permissions, true)?;
                        // Parse input: (string, option<value>, option<list<u8>>, option<u64>)
                        let (manifest_path, init_state_override, provided_wasm_bytes, timeout_ms) = match input {
                            Value::Tuple(mut args) if args.len() == 4 => {
                                let timeout_ms = parse_optional_u64(&args[3]);
                                let wasm_bytes = parse_optional_bytes(&args[2]);
                                let init_state_override = match args.remove(1) {
                                    Value::Option { value: None, .. } => None,
                                    Value::Option { value: Some(inner), .. } => Some(*inner),
                                    _ => return Err(Value::from(SupervisorError::InvalidArgument("init-state must be option<value>".to_string()))),
                                };
                                let manifest = match args.remove(0) {
                                    Value::String(s) => s,
                                    _ => return Err(Value::from(SupervisorError::InvalidArgument("invalid manifest argument".to_string()))),
                                };
                                (manifest, init_state_override, wasm_bytes, timeout_ms)
                            }
                            _ => return Err(Value::from(SupervisorError::InvalidArgument("expected (manifest, option<value>, option<list<u8>>, option<u64>)".to_string()))),
                        };

                        debug!("spawn-and-wait: manifest={}, timeout={:?}ms", manifest_path, timeout_ms);

                        // Load and parse manifest
                        let manifest_str = match resolve_reference(&manifest_path).await {
                            Ok(bytes) => match String::from_utf8(bytes) {
                                Ok(s) => s,
                                Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::BadManifest(format!("invalid manifest encoding: {}", e))))),
                            },
                            Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::BadManifest(format!("failed to load manifest: {}", e))))),
                        };

                        let manifest = match ManifestConfig::from_toml_str(&manifest_str) {
                            Ok(m) => m,
                            Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::BadManifest(format!("failed to parse manifest: {}", e))))),
                        };

                        // Resolve wasm bytes — same three paths as `spawn`.
                        // Emits `supervisor.wasm_resolve` so the
                        // used_cache / cache_hit observability story
                        // matches the `spawn` host fn — operators
                        // inspecting bench logs see the same fields on
                        // both variants.
                        let wasm_provided = provided_wasm_bytes.is_some();
                        let phase_start = Instant::now();
                        let mut used_cache = false;
                        let mut cache_hit = false;
                        let wasm_bytes = match provided_wasm_bytes {
                            Some(bytes) => bytes,
                            None => match (manifest.static_package, resource_cache.as_deref()) {
                                (true, Some(cache)) => {
                                    used_cache = true;
                                    match resolve_reference_cached(&manifest.package, cache).await {
                                        Ok((arc, hit)) => {
                                            cache_hit = hit;
                                            (*arc).clone()
                                        }
                                        Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::WasmFetch(format!("failed to load WASM: {}", e))))),
                                    }
                                }
                                _ => match resolve_reference(&manifest.package).await {
                                    Ok(bytes) => bytes,
                                    Err(e) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::WasmFetch(format!("failed to load WASM: {}", e))))),
                                },
                            },
                        };
                        info!(
                            phase = "supervisor.wasm_resolve",
                            manifest = %manifest_path,
                            bytes = wasm_bytes.len(),
                            provided = wasm_provided,
                            used_cache,
                            cache_hit,
                            elapsed_ms = phase_start.elapsed().as_millis() as u64,
                            "spawn phase complete",
                        );

                        let store = ctx.data();
                        let theater_tx = store.theater_tx.clone();

                        // Subscribe to the child's chain AT SPAWN (subscription_tx
                        // registers before init, so the terminal event can't be
                        // missed — no supervisor_tx / ActorResult needed).
                        let (ev_tx, mut ev_rx) = mpsc::channel::<(TheaterId, ChainEvent)>(64);

                        let name = Some(manifest.name.clone());
                        let (response_tx, response_rx) = oneshot::channel();
                        // Resolve init-state — same fallback as `spawn`.
                        let init_state = match init_state_override {
                            Some(v) => v,
                            None => match manifest.initial_state.as_ref() {
                                Some(s) => Value::String(s.clone()),
                                None => default_init_state(),
                            },
                        };
                        // Same as the regular spawn path — setup + auto-init.
                        let cmd = TheaterCommand::SpawnActor {
                            wasm_bytes,
                            name,
                            manifest: Some(manifest),
                            init_state,
                            response_tx,
                            subscription_tx: Some(ev_tx),
                            // This child is spawned by the calling actor.
                            parent_id: Some(store.id),
                        };

                        if theater_tx.send(cmd).is_err() {
                            return Err(Value::from(SupervisorError::RuntimeUnavailable));
                        }

                        // Wait for the actor to spawn
                        let actor_id = match response_rx.await {
                            Ok(Ok(id)) => id,
                            Ok(Err(e)) => return Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::from(e)))),
                            Err(_) => return Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        };

                        debug!("spawn-and-wait: child {} spawned, waiting for completion", actor_id);

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
                                debug!("spawn-and-wait: child {} completed successfully", actor_id);
                                Ok(option_bytes_to_value(final_state))
                            }
                            Ok(Some(TerminationCause::Failed { error })) => {
                                Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::ChildFailed(format!("child actor {} failed: {}", actor_id, error)))))
                            }
                            Ok(Some(cause)) => {
                                Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::ChildStopped(format!("child actor {} was stopped ({:?})", actor_id, cause)))))
                            }
                            Ok(None) => {
                                Err(Value::from(SupervisorError::Internal(format!("child actor {} chain closed before terminating", actor_id))))
                            }
                            Err(_) => {
                                // Timeout - stop the child actor
                                debug!("spawn-and-wait: timeout waiting for child {}, stopping it", actor_id);
                                let (stop_tx, _) = oneshot::channel();
                                let _ = theater_tx.send(TheaterCommand::StopActor {
                                    actor_id,
                                    response_tx: stop_tx,
                                });
                                Err(Value::from(SupervisorError::SpawnFailed(SpawnFailure::Timeout(format!("timeout waiting for child actor {} to complete", actor_id)))))
                            }
                        }
                    }
                }
            })?
            // list-actors: func() -> result<list<actor-info>, supervisor-error>
            // Every actor in the caller's view (subtree, or all).
            .func_async_result("list-actors", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let scope = require_capability(&permissions, false)?.scope;
                        let caller = ctx.data().id;
                        let tx = ctx.data().theater_tx.clone();
                        // Direct-children view-scope: `all` is every live actor
                        // (the runtime holds no lineage, so `parent-id` is `None`);
                        // `subtree` is the caller's own children, each reported with
                        // the caller as its parent. The full list is fetched once
                        // and filtered to `children` for the scoped case.
                        let actors = match scope {
                            ViewScope::All => get_actors(&tx).await?,
                            ViewScope::Subtree => {
                                let kids = children.lock().unwrap().clone();
                                get_actors(&tx)
                                    .await?
                                    .into_iter()
                                    .filter(|(id, _, _)| kids.contains(id))
                                    .map(|(id, name, _)| (id, name, Some(caller)))
                                    .collect()
                            }
                        };
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
            })?
            // get-actor-status: func(id: string) -> result<string, supervisor-error>
            .func_async_result("get-actor-status", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, false, &children).await?;
                        let (rtx, rrx) = oneshot::channel();
                        tx.send(TheaterCommand::GetActorStatus {
                            actor_id: target,
                            response_tx: rtx,
                        })
                        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(status)) => Ok(Value::String(format!("{:?}", status))),
                            Ok(Err(e)) => Err(Value::from(SupervisorError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // get-actor-state: func(id: string) -> result<option<list<u8>>, supervisor-error>
            .func_async_result("get-actor-state", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, false, &children).await?;
                        let (rtx, rrx) = oneshot::channel();
                        tx.send(TheaterCommand::GetActorState {
                            actor_id: target,
                            response_tx: rtx,
                        })
                        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(state)) => Ok(state),
                            Ok(Err(e)) => Err(Value::from(SupervisorError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // get-actor-manifest: func(id: string) -> result<string, supervisor-error>
            .func_async_result("get-actor-manifest", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, false, &children).await?;
                        let (rtx, rrx) = oneshot::channel();
                        tx.send(TheaterCommand::GetActorManifest {
                            actor_id: target,
                            response_tx: rtx,
                        })
                        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(m)) => serde_json::to_string(&m).map(Value::String).map_err(|e| {
                                SupervisorError::Internal(format!("serialize manifest: {}", e)).into()
                            }),
                            Ok(Err(e)) => Err(Value::from(SupervisorError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // get-actor-metrics: func(id: string) -> result<string, supervisor-error>
            .func_async_result("get-actor-metrics", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, false, &children).await?;
                        let (rtx, rrx) = oneshot::channel();
                        tx.send(TheaterCommand::GetActorMetrics {
                            actor_id: target,
                            response_tx: rtx,
                        })
                        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(m)) => serde_json::to_string(&m).map(Value::String).map_err(|e| {
                                SupervisorError::Internal(format!("serialize metrics: {}", e)).into()
                            }),
                            Ok(Err(e)) => Err(Value::from(SupervisorError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // stop-actor: func(id: string) -> result<_, supervisor-error>   (graceful)
            .func_async_result("stop-actor", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, true, &children).await?;
                        let (rtx, rrx) = oneshot::channel();
                        tx.send(TheaterCommand::StopActor {
                            actor_id: target,
                            response_tx: rtx,
                        })
                        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::from(SupervisorError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // kill-actor: func(id: string) -> result<_, supervisor-error>   (force)
            .func_async_result("kill-actor", {
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, true, &children).await?;
                        let (rtx, rrx) = oneshot::channel();
                        tx.send(TheaterCommand::TerminateActor {
                            actor_id: target,
                            response_tx: rtx,
                        })
                        .map_err(|_| SupervisorError::RuntimeUnavailable)?;
                        match rrx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::from(SupervisorError::Internal(e.to_string()))),
                            Err(_) => Err(Value::from(SupervisorError::RuntimeUnavailable)),
                        }
                    }
                }
            })?
            // subscribe-to-actor: func(id: string) -> result<_, supervisor-error>
            // Opt in to chain events from an actor in the caller's view. Events
            // are delivered to this actor's handle-actor-event export. Idempotent
            // (the chain identifies subscribers by Sender channel identity).
            .func_async_result("subscribe-to-actor", {
                let event_tx = event_tx.clone();
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let event_tx = event_tx.clone();
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let _ph = PhaseLog::new("supervisor.subscribe_to_actor");
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, false, &children).await?;
                        if tx
                            .send(TheaterCommand::SubscribeToActor {
                                actor_id: target,
                                event_tx,
                            })
                            .is_err()
                        {
                            return Err(Value::from(SupervisorError::RuntimeUnavailable));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            })?
            // unsubscribe-from-actor: func(id: string) -> result<_, supervisor-error>
            // Stop receiving chain events from an actor. Idempotent; also
            // auto-released when the actor exits.
            .func_async_result("unsubscribe-from-actor", {
                let event_tx = event_tx.clone();
                let permissions = permissions.clone();
                let children = children.clone();
                move |ctx: AsyncCtx<ActorStore>, id: String| {
                    let event_tx = event_tx.clone();
                    let permissions = permissions.clone();
                    let children = children.clone();
                    async move {
                        let _ph = PhaseLog::new("supervisor.unsubscribe_from_actor");
                        let target = parse_actor_id(&id)?;
                        let tx = ctx.data().theater_tx.clone();
                        authorize(&tx, &permissions, target, false, &children).await?;
                        if tx
                            .send(TheaterCommand::UnsubscribeFromActor {
                                actor_id: target,
                                event_tx,
                            })
                            .is_err()
                        {
                            return Err(Value::from(SupervisorError::RuntimeUnavailable));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            })?;

        ctx.mark_satisfied("theater:simple/supervisor");
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
        info!("Supervisor handler setup");

        // Take the event receiver out of the Arc<Mutex<Option<>>>
        let event_rx_opt = self.event_rx.lock().unwrap().take();

        // Clone children and theater_tx for use in the async closure
        let children = self.children.clone();
        let theater_tx = self.theater_tx.clone();

        Box::pin(async move {
            // If we don't have a receiver (e.g., this is a cloned instance), just wait for shutdown
            let Some(mut event_rx) = event_rx_opt else {
                info!("Supervisor handler has no receiver (cloned instance), not starting");
                // Still need to wait for shutdown to avoid blocking the shutdown controller
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            // Which optional callbacks does the actor implement?
            let (has_child_event, has_lifecycle) = {
                let mut instance_guard = actor_instance.write().await;
                if let Some(instance) = instance_guard.as_mut() {
                    let iface = "theater:simple/supervisor-handlers";
                    let e1 = instance
                        .has_export(iface, "handle-actor-event")
                        .await
                        .unwrap_or(false);
                    let e2 = instance
                        .has_export(iface, "handle-lifecycle-event")
                        .await
                        .unwrap_or(false);
                    (e1, e2)
                } else {
                    (false, false)
                }
            };

            if has_child_event || has_lifecycle {
                debug!(
                    "Supervisor handler exports: event={}, lifecycle={}",
                    has_child_event, has_lifecycle
                );
            }

            loop {
                tokio::select! {
                    // Chain events from watched actors — each tagged with its
                    // source TheaterId. A terminal drives handle-lifecycle-event +
                    // tracking cleanup; other events drive handle-actor-event.
                    Some(event_with_id) = event_rx.recv() => {
                        if let Err(e) = Self::process_child_event(
                            &actor_handle, event_with_id, &children, has_child_event, has_lifecycle,
                        ).await {
                            error!("Error processing child event: {}", e);
                        }
                    }
                    _ = &mut shutdown_receiver.receiver => {
                        info!("Shutdown signal received, stopping children");
                        break;
                    }
                }
            }

            // Stop all children on shutdown
            let children_to_stop: Vec<TheaterId> = {
                let children_guard = children.lock().unwrap();
                children_guard.iter().cloned().collect()
            };

            if !children_to_stop.is_empty() {
                info!("Stopping {} children", children_to_stop.len());

                // Get theater_tx if available
                let theater_tx_opt = {
                    let guard = theater_tx.lock().unwrap();
                    guard.clone()
                };

                if let Some(theater_tx) = theater_tx_opt {
                    for child_id in &children_to_stop {
                        debug!("Stopping child {}", child_id);
                        let (response_tx, _response_rx) = oneshot::channel();
                        let cmd = TheaterCommand::StopActor {
                            actor_id: *child_id,
                            response_tx,
                        };
                        if let Err(e) = theater_tx.send(cmd) {
                            warn!("Failed to send stop command for child {}: {}", child_id, e);
                        }
                    }

                    // Wait for children to exit (with timeout)
                    let timeout = Duration::from_secs(5);
                    let start = std::time::Instant::now();

                    while start.elapsed() < timeout {
                        // Check if all children have exited
                        let remaining = {
                            let children_guard = children.lock().unwrap();
                            children_guard.len()
                        };

                        if remaining == 0 {
                            info!("All children have exited");
                            break;
                        }

                        // Drain terminal events so children leave tracking as they die.
                        tokio::select! {
                            Some(event_with_id) = event_rx.recv() => {
                                if let Err(e) = Self::process_child_event(
                                    &actor_handle, event_with_id, &children, has_child_event, has_lifecycle,
                                ).await {
                                    error!("Error processing child event during shutdown: {}", e);
                                }
                            }
                            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                                // Just check again
                            }
                        }
                    }

                    let remaining = {
                        let children_guard = children.lock().unwrap();
                        children_guard.len()
                    };
                    if remaining > 0 {
                        warn!("{} children did not exit within timeout", remaining);
                    }
                } else {
                    warn!("No theater_tx available, cannot stop children");
                }
            }

            info!("Supervisor handler shut down complete");
            Ok(())
        })
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
        Value::Option { value: None, .. } => None,
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
        Value::Option { value: None, .. } => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::SupervisorHostConfig;

    #[test]
    fn test_supervisor_handler_creation() {
        let config = SupervisorHostConfig {};
        let handler = SupervisorHandler::new(config, None);
        assert_eq!(handler.name(), "supervisor");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/supervisor".to_string()])
        );
        assert_eq!(
            handler.exports(),
            Some(vec!["theater:simple/supervisor-handlers".to_string()])
        );
    }

    #[test]
    fn test_supervisor_handler_clone() {
        let config = SupervisorHostConfig {};
        let handler = SupervisorHandler::new(config, None);
        let cloned = handler.create_instance(None);
        assert_eq!(cloned.name(), "supervisor");
    }

    #[test]
    fn test_supervisor_interface_hash_determinism() {
        let interface1 = supervisor_interface();
        let interface2 = supervisor_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_create_instance_yields_independent_supervisors() {
        // Regression: the derived `Clone` shared event_rx/children across
        // every supervisor-capable actor, so only the first `setup` won the
        // `event_rx.take()` and ran the single monitor loop — collapsing
        // multi-level supervision trees to one root-only supervisor. Each
        // per-actor instance must instead be fully independent.
        let base = SupervisorHandler::new(SupervisorHostConfig {}, None);
        let a = base.fresh();
        let b = base.fresh();

        // Each instance owns its own (untaken) event receiver, so each one's
        // monitor loop will start rather than log "no receiver".
        assert!(a.event_rx.lock().unwrap().is_some());
        assert!(b.event_rx.lock().unwrap().is_some());

        // Distinct event channels: a child auto-subscribed by one supervisor
        // must route only to that supervisor's loop.
        assert!(!a.event_tx.same_channel(&b.event_tx));
        assert!(!a.event_tx.same_channel(&base.event_tx));

        // Independent children sets: one supervisor tracking a child must not
        // leak that child into another supervisor's view.
        a.children.lock().unwrap().insert(TheaterId::generate());
        assert_eq!(a.children.lock().unwrap().len(), 1);
        assert_eq!(b.children.lock().unwrap().len(), 0);
        assert_eq!(base.children.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_supervisor_handler_interface_hashes() {
        let config = SupervisorHostConfig {};
        let handler = SupervisorHandler::new(config, None);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/supervisor");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }
}
