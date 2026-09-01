//! # Lifecycle-Relationship Handler
//!
//! The actor-facing surface over lifecycle relationships. An actor attaches a
//! directed relationship to another actor (the *subject*), always as itself
//! (self-service):
//!
//! - [`link`] / `unlink` — **fate-sharing**: when the subject terminates, the
//!   linking actor is stopped (cause `PeerKilled`).
//! - [`monitor`] / `unmonitor` — **watching**: the subject's matching events are
//!   delivered to the actor's `handle-lifecycle-event` export.
//!
//! Both are the *same* mechanism: `link`/`monitor` subscribe this actor's
//! handler to the subject's chain (via `SubscribeToActor`) and record a
//! `{ filter, target }`. The handler's loop matches every subject event
//! host-side (`packr_abi::Pattern`) and **acts by target** — `StopSelf` → ask
//! the runtime to stop this actor (`PeerTerminated`), `DeliverToWasm` → call the
//! export. The runtime is not in this path; fate and watching both flow chain →
//! handler → (stop | wasm).
//!
//! v1 filters are fixed (link → any termination, monitor → any lifecycle
//! event); custom `packr_abi::Pattern` filters follow once patterns cross the
//! wasm↔host boundary as a pact type.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::chain::ChainEvent;
use theater::events::decode_chain_event_payload;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use theater::subscription::{any_lifecycle_event, any_termination, Target};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, Pattern, TypeHash, Value,
    ValueType,
};

/// Import side: `link`/`monitor` host functions the actor calls.
const LIFECYCLE_PACT: &str = include_str!("../lifecycle.pact");
/// Export side: the `handle-lifecycle-event` callback the actor implements. The
/// canonical contract source; exports are matched by name (`has_export`), so
/// this is consumed by the test + downstream actors rather than the handler.
#[allow(dead_code)]
const LIFECYCLE_HANDLERS_PACT: &str = include_str!("../lifecycle-handlers.pact");

/// A relationship this actor holds against one subject: a match-any filter and
/// what to do on a match.
struct LocalSub {
    filter: Vec<Pattern>,
    target: Target,
}

/// subject → this actor's subscriptions on it. One chain subscription per
/// subject serves all of them (subscribe on the first, unsubscribe on the last).
type Subs = Arc<Mutex<HashMap<TheaterId, Vec<LocalSub>>>>;
/// The monitor/link delivery receiver, taken by `setup` once.
type EventRx = Arc<Mutex<Option<mpsc::Receiver<(TheaterId, ChainEvent)>>>>;

fn lifecycle_interface() -> InterfaceImpl {
    let pact = parse_pact(LIFECYCLE_PACT).expect("embedded lifecycle.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

#[allow(dead_code)]
fn lifecycle_handlers_interface() -> InterfaceImpl {
    let pact = parse_pact(LIFECYCLE_HANDLERS_PACT)
        .expect("embedded lifecycle-handlers.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// The set of chain event-types that count as lifecycle events. Non-lifecycle
/// chain events (wasm/host-function) are cheaply skipped before any decode.
const LIFECYCLE_EVENT_TYPES: &[&str] = &["spawned", "paused", "resumed", "terminated"];

/// Handler providing `theater:simple/lifecycle` (link / monitor) to actors.
///
/// Per-actor state is **fresh per instance** — `create_instance` builds a new
/// one rather than cloning shared `Arc`s (cf. the handler-clone-shares-state
/// trap).
pub struct LifecycleHandler {
    theater_tx: UnboundedSender<TheaterCommand>,
    /// Chain-subscriber sender handed to the subjects this actor relates to;
    /// their events arrive on `event_rx` (drained by `setup`).
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
    event_rx: EventRx,
    /// This actor's relationships, keyed by subject.
    subs: Subs,
    /// This actor's own id, learned when it first calls link/monitor — needed to
    /// name itself in a `StopSelf` (`PeerTerminated`) request.
    self_id: Arc<Mutex<Option<TheaterId>>>,
}

impl LifecycleHandler {
    pub fn new(theater_tx: UnboundedSender<TheaterCommand>) -> Self {
        Self::fresh(theater_tx)
    }

    fn fresh(theater_tx: UnboundedSender<TheaterCommand>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            theater_tx,
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            subs: Arc::new(Mutex::new(HashMap::new())),
            self_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![lifecycle_interface()]
    }
}

/// Parse the `subject` string argument (an actor id) from a lifecycle call.
fn parse_subject(input: &Value) -> Result<TheaterId, Value> {
    match input {
        Value::String(s) => TheaterId::from_str(s)
            .map_err(|_| Value::String(format!("invalid subject actor id: {s}"))),
        _ => Err(Value::String("expected subject actor id (string)".into())),
    }
}

impl Handler for LifecycleHandler {
    fn create_instance(
        &self,
        _config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(Self::fresh(self.theater_tx.clone()))
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        actor_instance: SharedActorInstance,
        mut shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        let event_rx_opt = self.event_rx.lock().unwrap().take();
        let subs = self.subs.clone();
        let self_id = self.self_id.clone();
        let theater_tx = self.theater_tx.clone();

        Box::pin(async move {
            let Some(mut event_rx) = event_rx_opt else {
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            // Does this actor implement the monitor callback? (Only gates the
            // DeliverToWasm arm; StopSelf needs no export.)
            let has_callback = {
                let mut guard = actor_instance.write().await;
                match guard.as_mut() {
                    Some(inst) => inst
                        .has_export(
                            "theater:simple/lifecycle-handlers",
                            "handle-lifecycle-event",
                        )
                        .await
                        .unwrap_or(false),
                    None => false,
                }
            };

            loop {
                tokio::select! {
                    Some((subject_id, event)) = event_rx.recv() => {
                        // Cheap event-type pre-filter (v1 filters are lifecycle-only).
                        if !LIFECYCLE_EVENT_TYPES.contains(&event.event_type.as_str()) {
                            continue;
                        }
                        let value = match decode_chain_event_payload(&event.data) {
                            Some(payload) => Value::from(payload),
                            None => continue,
                        };
                        // Collect the matched targets, then drop the lock before
                        // any await.
                        let matched: Vec<Target> = {
                            let subs = subs.lock().unwrap();
                            subs.get(&subject_id)
                                .map(|list| {
                                    list.iter()
                                        .filter(|s| s.filter.iter().any(|p| p.matches(&value)))
                                        .map(|s| s.target.clone())
                                        .collect()
                                })
                                .unwrap_or_default()
                        };
                        for target in matched {
                            match target {
                                Target::StopSelf => {
                                    // Fate: this actor is peer-killed by `subject_id`.
                                    let me = *self_id.lock().unwrap();
                                    if let Some(me) = me {
                                        let _ = theater_tx.send(TheaterCommand::PeerTerminated {
                                            actor_id: me,
                                            peer: subject_id,
                                        });
                                    }
                                }
                                Target::DeliverToWasm => {
                                    if has_callback {
                                        let params = Value::Tuple(vec![
                                            Value::String(subject_id.to_string()),
                                            Value::String(event.event_type.clone()),
                                            Value::List {
                                                elem_type: ValueType::U8,
                                                items: event.data.iter().map(|b| Value::U8(*b)).collect(),
                                            },
                                        ]);
                                        if let Err(e) = actor_handle
                                            .call_function(
                                                "theater:simple/lifecycle-handlers.handle-lifecycle-event"
                                                    .to_string(),
                                                params,
                                            )
                                            .await
                                        {
                                            error!("handle-lifecycle-event delivery failed: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = &mut shutdown_receiver.receiver => {
                        break;
                    }
                }
            }
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        if ctx.is_satisfied("theater:simple/lifecycle") {
            return Ok(());
        }

        let link = (
            self.theater_tx.clone(),
            self.event_tx.clone(),
            self.subs.clone(),
            self.self_id.clone(),
        );
        let monitor = (
            self.theater_tx.clone(),
            self.event_tx.clone(),
            self.subs.clone(),
            self.self_id.clone(),
        );
        let unlink = (
            self.theater_tx.clone(),
            self.event_tx.clone(),
            self.subs.clone(),
        );
        let unmonitor = (
            self.theater_tx.clone(),
            self.event_tx.clone(),
            self.subs.clone(),
        );

        builder
            .interface("theater:simple/lifecycle")?
            // link(subject) -> result<_, string>
            .func_async_result("link", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let (theater_tx, event_tx, subs, self_id) = link.clone();
                async move {
                    add_subscription(
                        &ctx,
                        &input,
                        &theater_tx,
                        event_tx,
                        &subs,
                        &self_id,
                        vec![any_termination()],
                        Target::StopSelf,
                    )
                }
            })?
            // monitor(subject) -> result<_, string>
            .func_async_result("monitor", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let (theater_tx, event_tx, subs, self_id) = monitor.clone();
                async move {
                    add_subscription(
                        &ctx,
                        &input,
                        &theater_tx,
                        event_tx,
                        &subs,
                        &self_id,
                        vec![any_lifecycle_event()],
                        Target::DeliverToWasm,
                    )
                }
            })?
            // unlink(subject) -> result<_, string>
            .func_async_result("unlink", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let (theater_tx, event_tx, subs) = unlink.clone();
                async move {
                    remove_subscription(&input, &theater_tx, event_tx, &subs, Target::StopSelf)
                }
            })?
            // unmonitor(subject) -> result<_, string>
            .func_async_result(
                "unmonitor",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let (theater_tx, event_tx, subs) = unmonitor.clone();
                    async move {
                        remove_subscription(
                            &input,
                            &theater_tx,
                            event_tx,
                            &subs,
                            Target::DeliverToWasm,
                        )
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/lifecycle");
        info!("lifecycle handler host functions registered");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "lifecycle"
    }

    fn imports(&self) -> Option<Vec<String>> {
        let mut imports: Vec<String> = self
            .interfaces()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        imports.push("theater:simple/types".to_string());
        Some(imports)
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/lifecycle-handlers".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![lifecycle_interface()]
    }
}

/// Record a `{filter, target}` for the caller against `subject`, subscribing
/// this handler to the subject's chain on the first subscription to it.
#[allow(clippy::too_many_arguments)]
fn add_subscription(
    ctx: &AsyncCtx<ActorStore>,
    input: &Value,
    theater_tx: &UnboundedSender<TheaterCommand>,
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
    subs: &Subs,
    self_id: &Arc<Mutex<Option<TheaterId>>>,
    filter: Vec<Pattern>,
    target: Target,
) -> Result<Value, Value> {
    let subject = parse_subject(input)?;
    *self_id.lock().unwrap() = Some(ctx.data().id);
    let first = {
        let mut subs = subs.lock().unwrap();
        let entry = subs.entry(subject).or_default();
        let first = entry.is_empty();
        entry.push(LocalSub { filter, target });
        first
    };
    if first {
        let _ = theater_tx.send(TheaterCommand::SubscribeToActor {
            actor_id: subject,
            event_tx,
        });
    }
    Ok(Value::Tuple(vec![]))
}

/// Drop the caller's subscriptions of `target` on `subject`, unsubscribing from
/// its chain once none remain.
fn remove_subscription(
    input: &Value,
    theater_tx: &UnboundedSender<TheaterCommand>,
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
    subs: &Subs,
    target: Target,
) -> Result<Value, Value> {
    let subject = parse_subject(input)?;
    let now_empty = {
        let mut subs = subs.lock().unwrap();
        if let Some(entry) = subs.get_mut(&subject) {
            entry.retain(|s| s.target != target);
            if entry.is_empty() {
                subs.remove(&subject);
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if now_empty {
        let _ = theater_tx.send(TheaterCommand::UnsubscribeFromActor {
            actor_id: subject,
            event_tx,
        });
    }
    Ok(Value::Tuple(vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_name_and_interface_hashes_are_stable() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let handler = LifecycleHandler::new(tx);
        assert_eq!(handler.name(), "lifecycle");
        assert_eq!(lifecycle_interface().hash(), lifecycle_interface().hash());
    }

    #[test]
    fn both_interfaces_parse() {
        assert_eq!(lifecycle_interface().name(), "theater:simple/lifecycle");
        assert_eq!(
            lifecycle_handlers_interface().name(),
            "theater:simple/lifecycle-handlers"
        );
    }
}
