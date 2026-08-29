//! Theater Runtime Control Handler
//!
//! Provides the runtime-wide CONTROL interface (`theater:simple/runtime`):
//! inspect and drive ANY actor by id ("root in Linux"). This replaces the
//! deleted theater-server management socket — a console/sentinel actor imports
//! this interface to BE the server, recording every control op in its own chain.
//!
//! Capability-gated by [`RuntimePermissions`] `{ inspect, mutate }` — the first
//! real runtime-side permission enforcement. inspect = list/get/subscribe;
//! mutate = spawn/stop/kill/restart. History is NOT served here: the
//! runtime retains no chain, so a subscriber accumulates chain events live via
//! `subscribe-to-actor` (delivered to its `handle-actor-event` export) and
//! persists them itself. A terminal event always reaches subscribers before the
//! actor's chain closes.

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::config::permissions::RuntimePermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::{default_init_state, TheaterCommand};
use theater::shutdown::ShutdownReceiver;
use theater::utils::{resolve_reference, resolve_reference_cached, ResourceCache};
use theater::ManifestConfig;

use theater::chain::ChainEvent;
use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

use anyhow::Result;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use theater::id::TheaterId;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

/// Embedded runtime.pact file content
const RUNTIME_PACT: &str = include_str!("../runtime.pact");

/// Declare the theater:simple/runtime CONTROL interface from the pact file.
fn runtime_interface() -> InterfaceImpl {
    let pact = parse_pact(RUNTIME_PACT).expect("embedded runtime.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// The RuntimeHandler exposes the runtime-wide control plane to a granted actor.
///
/// NOTE on `Clone`: like the supervisor handler, the derived `Clone` shares the
/// event channel + subscription set via `Arc`/`Sender`. Per-actor instantiation
/// MUST go through [`Self::fresh`] (which `create_instance` calls) so each
/// control-capable actor gets its own event loop and subscription set. Do not
/// swap that to `self.clone()`.
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct RuntimeHandler {
    /// Chain events from every actor this handler is subscribed to, tagged with
    /// the source actor's id so `handle-actor-event` dispatch can attribute it.
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
    event_rx: Arc<Mutex<Option<mpsc::Receiver<(TheaterId, ChainEvent)>>>>,
    /// Actors this handler is currently subscribed to (for cleanup on shutdown).
    subscribed: Arc<Mutex<HashSet<TheaterId>>>,
    /// Optional shared URL-bytes cache for `spawn` of `static_package` actors.
    resource_cache: Option<Arc<ResourceCache>>,
    /// The granted control capability. `None` = not granted = every op denied
    /// (default-deny; the god-mode-socket hole is closed here). Populated per
    /// actor from its manifest grant when the handler is registered.
    permissions: Option<RuntimePermissions>,
}

impl RuntimeHandler {
    /// Create a new RuntimeHandler with the actor's granted control capability.
    pub fn new(_config: RuntimeHostConfig, permissions: Option<RuntimePermissions>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            subscribed: Arc::new(Mutex::new(HashSet::new())),
            resource_cache: None,
            permissions,
        }
    }

    /// Wire in a shared `ResourceCache` so `spawn` of `static_package` actors
    /// skips the wasm-bytes fetch on repeat calls.
    pub fn with_resource_cache(mut self, cache: Arc<ResourceCache>) -> Self {
        self.resource_cache = Some(cache);
        self
    }

    /// Build a genuinely independent instance: fresh event channel + empty
    /// subscription set, carrying forward only process-global config (the
    /// cache and the permissions). See the `Clone` note on the struct.
    fn fresh(&self) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            subscribed: Arc::new(Mutex::new(HashSet::new())),
            resource_cache: self.resource_cache.clone(),
            permissions: self.permissions.clone(),
        }
    }

    /// Dispatch one chain event to the actor's `handle-actor-event` export
    /// (mirrors the supervisor's `handle-child-event`). params =
    /// (actor-id, event-type, data).
    async fn process_actor_event(
        actor_handle: &ActorHandle,
        event_with_id: (TheaterId, ChainEvent),
        has_actor_event: bool,
    ) -> Result<()> {
        let (actor_id, event) = event_with_id;
        if has_actor_event {
            let params = Value::Tuple(vec![
                Value::String(actor_id.to_string()),
                Value::String(event.event_type.clone()),
                Value::List {
                    elem_type: ValueType::U8,
                    items: event.data.iter().map(|b| Value::U8(*b)).collect(),
                },
            ]);
            actor_handle
                .call_function(
                    "theater:simple/runtime-handlers.handle-actor-event".to_string(),
                    params,
                )
                .await?;
        }
        Ok(())
    }
}

/// Enforce the control capability. `mutate = false` needs `inspect`,
/// `mutate = true` needs `mutate`. Default-deny when the capability is absent.
fn require(perms: &Option<RuntimePermissions>, mutate: bool) -> Result<(), Value> {
    let granted = match perms {
        Some(p) => {
            if mutate {
                p.mutate
            } else {
                p.inspect
            }
        }
        None => false,
    };
    if granted {
        Ok(())
    } else {
        Err(Value::String(format!(
            "permission denied: theater:simple/runtime requires the '{}' capability",
            if mutate { "mutate" } else { "inspect" }
        )))
    }
}

/// Parse a single `id: string` argument (bare or as a 1-tuple).
fn parse_id_arg(input: Value, op: &str) -> Result<TheaterId, Value> {
    let id_str = match input {
        Value::String(s) => s,
        Value::Tuple(args) if args.len() == 1 => match &args[0] {
            Value::String(s) => s.clone(),
            _ => return Err(Value::String(format!("Invalid id argument to {}", op))),
        },
        _ => return Err(Value::String(format!("Invalid argument to {}", op))),
    };
    id_str
        .parse()
        .map_err(|e| Value::String(format!("Invalid actor id: {}", e)))
}

/// Extract owned bytes from an `option<list<u8>>` / `list<u8>` Value.
fn parse_optional_bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Option {
            value: Some(inner), ..
        } => bytes_from_list(inner),
        Value::Option { value: None, .. } => None,
        Value::List { .. } => bytes_from_list(v),
        _ => None,
    }
}

fn bytes_from_list(v: &Value) -> Option<Vec<u8>> {
    if let Value::List { items, .. } = v {
        Some(
            items
                .iter()
                .filter_map(|i| if let Value::U8(b) = i { Some(*b) } else { None })
                .collect(),
        )
    } else {
        None
    }
}

impl Handler for RuntimeHandler {
    fn create_instance(
        &self,
        _config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(self.fresh())
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

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up runtime control host functions (Pack)");
        if ctx.is_satisfied("theater:simple/runtime") {
            info!("theater:simple/runtime already satisfied by another handler, skipping");
            return Ok(());
        }

        let event_tx = self.event_tx.clone();
        let subscribed = self.subscribed.clone();
        let resource_cache = self.resource_cache.clone();
        let permissions = self.permissions.clone();

        builder
            .interface("theater:simple/runtime")?
            // ---- INSPECT ----
            // list-actors: func() -> result<list<actor-info>, string>
            .func_async_result("list-actors", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActors { response_tx })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(actors)) => {
                                let items = actors
                                    .into_iter()
                                    .map(|(id, name, parent)| Value::Record {
                                        type_name: "actor-info".to_string(),
                                        fields: vec![
                                            ("id".to_string(), Value::String(id.to_string())),
                                            ("name".to_string(), Value::String(name)),
                                            (
                                                "parent-id".to_string(),
                                                Value::Option {
                                                    inner_type: ValueType::String,
                                                    value: parent.map(|p| {
                                                        Box::new(Value::String(p.to_string()))
                                                    }),
                                                },
                                            ),
                                        ],
                                    })
                                    .collect();
                                Ok(Value::List {
                                    elem_type: ValueType::Record("actor-info".to_string()),
                                    items,
                                })
                            }
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // get-actor-status: func(id: string) -> result<string, string>
            .func_async_result("get-actor-status", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let actor_id = parse_id_arg(input, "get-actor-status")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorStatus {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(status)) => Ok(Value::String(format!("{:?}", status))),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // get-actor-state: func(id: string) -> result<option<list<u8>>, string>
            .func_async_result("get-actor-state", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let actor_id = parse_id_arg(input, "get-actor-state")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorState {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(state)) => Ok(state),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // get-actor-manifest: func(id: string) -> result<string, string>
            .func_async_result("get-actor-manifest", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let actor_id = parse_id_arg(input, "get-actor-manifest")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorManifest {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(manifest)) => match serde_json::to_string(&manifest) {
                                Ok(s) => Ok(Value::String(s)),
                                Err(e) => Err(Value::String(format!("serialize manifest: {}", e))),
                            },
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // get-actor-metrics: func(id: string) -> result<string, string>
            .func_async_result("get-actor-metrics", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let actor_id = parse_id_arg(input, "get-actor-metrics")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::GetActorMetrics {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(metrics)) => match serde_json::to_string(&metrics) {
                                Ok(s) => Ok(Value::String(s)),
                                Err(e) => Err(Value::String(format!("serialize metrics: {}", e))),
                            },
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // subscribe-to-actor: func(id: string) -> result<_, string>
            .func_async_result("subscribe-to-actor", {
                let permissions = permissions.clone();
                let event_tx = event_tx.clone();
                let subscribed = subscribed.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    let event_tx = event_tx.clone();
                    let subscribed = subscribed.clone();
                    async move {
                        require(&permissions, false)?;
                        let actor_id = parse_id_arg(input, "subscribe-to-actor")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        if let Err(e) = theater_tx
                            .send(TheaterCommand::SubscribeToActor { actor_id, event_tx })
                            .await
                        {
                            return Err(Value::String(format!("Failed to send: {}", e)));
                        }
                        subscribed.lock().unwrap().insert(actor_id);
                        Ok(Value::Tuple(vec![]))
                    }
                }
            })?
            // unsubscribe-from-actor: func(id: string) -> result<_, string>
            .func_async_result("unsubscribe-from-actor", {
                let permissions = permissions.clone();
                let event_tx = event_tx.clone();
                let subscribed = subscribed.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    let event_tx = event_tx.clone();
                    let subscribed = subscribed.clone();
                    async move {
                        require(&permissions, false)?;
                        let actor_id = parse_id_arg(input, "unsubscribe-from-actor")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        if let Err(e) = theater_tx
                            .send(TheaterCommand::UnsubscribeFromActor { actor_id, event_tx })
                            .await
                        {
                            return Err(Value::String(format!("Failed to send: {}", e)));
                        }
                        subscribed.lock().unwrap().remove(&actor_id);
                        Ok(Value::Tuple(vec![]))
                    }
                }
            })?
            // ---- MUTATE ----
            // spawn: func(manifest, option<value>, option<list<u8>>) -> result<string, string>
            .func_async_result("spawn", {
                let permissions = permissions.clone();
                let resource_cache = resource_cache.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    let resource_cache = resource_cache.clone();
                    async move {
                        require(&permissions, true)?;
                        let (manifest_path, init_state_override, provided_wasm_bytes) = match input {
                            Value::Tuple(mut args) if args.len() == 3 => {
                                let wasm_bytes = parse_optional_bytes(&args[2]);
                                let init_state_override = match args.remove(1) {
                                    Value::Option { value: None, .. } => None,
                                    Value::Option {
                                        value: Some(inner), ..
                                    } => Some(*inner),
                                    _ => {
                                        return Err(Value::String(
                                            "Invalid init-state: expected option<value>".to_string(),
                                        ))
                                    }
                                };
                                let manifest = match args.remove(0) {
                                    Value::String(s) => s,
                                    _ => {
                                        return Err(Value::String(
                                            "Invalid manifest argument".to_string(),
                                        ))
                                    }
                                };
                                (manifest, init_state_override, wasm_bytes)
                            }
                            _ => {
                                return Err(Value::String(
                                    "Invalid spawn arguments: expected (string, option<value>, option<list<u8>>)"
                                        .to_string(),
                                ))
                            }
                        };

                        let manifest_str = match resolve_reference(&manifest_path).await {
                            Ok(bytes) => String::from_utf8(bytes)
                                .map_err(|e| Value::String(format!("Invalid manifest encoding: {}", e)))?,
                            Err(e) => return Err(Value::String(format!("Failed to load manifest: {}", e))),
                        };
                        let manifest = ManifestConfig::from_toml_str(&manifest_str)
                            .map_err(|e| Value::String(format!("Failed to parse manifest: {}", e)))?;

                        let wasm_bytes = match provided_wasm_bytes {
                            Some(bytes) => bytes,
                            None => match (manifest.static_package, resource_cache.as_deref()) {
                                (true, Some(cache)) => match resolve_reference_cached(&manifest.package, cache).await {
                                    Ok((arc, _hit)) => (*arc).clone(),
                                    Err(e) => return Err(Value::String(format!("Failed to load WASM: {}", e))),
                                },
                                _ => match resolve_reference(&manifest.package).await {
                                    Ok(bytes) => bytes,
                                    Err(e) => return Err(Value::String(format!("Failed to load WASM: {}", e))),
                                },
                            },
                        };

                        let init_state = match init_state_override {
                            Some(v) => v,
                            None => match manifest.initial_state.as_ref() {
                                Some(s) => Value::String(s.clone()),
                                None => default_init_state(),
                            },
                        };

                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::SpawnActor {
                                wasm_bytes,
                                name: Some(manifest.name.clone()),
                                manifest: Some(manifest),
                                init_state,
                                response_tx,
                                supervisor_tx: None,
                                subscription_tx: None,
                                // Control-plane spawns are top-level (root) actors.
                                parent_id: None,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send spawn: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(id)) => Ok(Value::String(id.to_string())),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // stop-actor: func(id: string) -> result<_, string>   (graceful)
            .func_async_result("stop-actor", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, true)?;
                        let actor_id = parse_id_arg(input, "stop-actor")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::StopActor {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // kill-actor: func(id: string) -> result<_, string>   (forceful)
            .func_async_result("kill-actor", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, true)?;
                        let actor_id = parse_id_arg(input, "kill-actor")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::TerminateActor {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?
            // restart-actor: func(id: string) -> result<_, string>
            .func_async_result("restart-actor", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, true)?;
                        let actor_id = parse_id_arg(input, "restart-actor")?;
                        let theater_tx = ctx.data().theater_tx.clone();
                        let (response_tx, response_rx) = oneshot::channel();
                        theater_tx
                            .send(TheaterCommand::RestartActor {
                                actor_id,
                                response_tx,
                            })
                            .await
                            .map_err(|e| Value::String(format!("Failed to send: {}", e)))?;
                        match response_rx.await {
                            Ok(Ok(())) => Ok(Value::Tuple(vec![])),
                            Ok(Err(e)) => Err(Value::String(e.to_string())),
                            Err(e) => Err(Value::String(format!("Failed to receive: {}", e))),
                        }
                    }
                }
            })?;

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
        info!("Runtime control handler setup");

        let event_rx_opt = self.event_rx.lock().unwrap().take();
        let subscribed = self.subscribed.clone();

        Box::pin(async move {
            let Some(mut event_rx) = event_rx_opt else {
                info!("Runtime handler has no receiver (cloned instance), not starting");
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            // Does the actor implement the chain-event ingest export?
            let has_actor_event = {
                let mut instance_guard = actor_instance.write().await;
                if let Some(instance) = instance_guard.as_mut() {
                    instance
                        .has_export("theater:simple/runtime-handlers", "handle-actor-event")
                        .await
                        .unwrap_or(false)
                } else {
                    false
                }
            };

            loop {
                tokio::select! {
                    Some(event_with_id) = event_rx.recv() => {
                        if let Err(e) = Self::process_actor_event(
                            &actor_handle, event_with_id, has_actor_event,
                        ).await {
                            error!("Error processing actor event: {}", e);
                        }
                    }
                    _ = &mut shutdown_receiver.receiver => {
                        debug!("Runtime handler shutdown");
                        break;
                    }
                }
            }

            subscribed.lock().unwrap().clear();
            Ok(())
        })
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
        // None => every op denied (default-deny).
        assert!(require(&None, false).is_err());
        assert!(require(&None, true).is_err());
        // inspect-only grant: inspect ok, mutate denied.
        let ro = Some(RuntimePermissions {
            inspect: true,
            mutate: false,
        });
        assert!(require(&ro, false).is_ok());
        assert!(require(&ro, true).is_err());
        // full grant: both ok.
        let rw = Some(RuntimePermissions {
            inspect: true,
            mutate: true,
        });
        assert!(require(&rw, false).is_ok());
        assert!(require(&rw, true).is_ok());
    }
}
