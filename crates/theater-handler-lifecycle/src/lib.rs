//! # Lifecycle-Relationship Handler
//!
//! The actor-facing surface over lifecycle relationships. An actor attaches a
//! directed relationship to another actor (the *subject*), always as itself
//! (self-service):
//!
//! - [`link`] / `unlink` — **fate-sharing**: when the subject terminates, the
//!   linking actor is stopped (cause `PeerKilled`).
//! - [`monitor`] / `unmonitor` — **watching**: the subject's whole chain is
//!   delivered to the actor's `handle-actor-event` export.
//!
//! Both are the *same* mechanism: `link`/`monitor` subscribe this actor's
//! handler to the subject's chain (via `SubscribeToActor`) and record a
//! `{ filter, target }`. The handler's loop matches every subject event
//! host-side (`packr_abi::Pattern`) and **acts by target** — `StopSelf` → ask
//! the runtime to stop this actor (`PeerTerminated`), `DeliverToWasm` → call the
//! export. The runtime is not in this path; fate and watching both flow chain →
//! handler → (stop | wasm).
//!
//! `link` keys on any termination; a bare `monitor` watches the **whole chain**
//! (a match-all `Pattern`, so every chain event of the subject is delivered).
//! [`monitor-filtered`](Handler) lets a caller supply its own `packr_abi::Pattern`
//! (crossing the wasm↔host boundary as a serialized `value`) to narrow the
//! delivered stream — otherwise identical to `monitor`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

use theater::actor::handle::ActorHandle;
use theater::chain::ChainEvent;
use theater::events::decode_chain_event_payload;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use theater::subscription::{any_termination, Target};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;

use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, InterfaceImpl, Pattern, TypeHash, Value, ValueType,
};

/// Import side: `link`/`monitor` host functions the actor calls.
const LIFECYCLE_PACT: &str = include_str!("../lifecycle.pact");
/// Export side: the `handle-actor-event` callback the actor implements. The
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

/// Split a `monitor-filtered(subject: string, filter: value)` call into its
/// subject (kept as a `Value::String` for [`add_subscription`] to re-parse, same
/// as `monitor`) and a decoded [`Pattern`]. `filter` is a serialized
/// `packr_abi::Pattern`, decoded via its `TryFrom<Value>`; a malformed pattern
/// becomes a clean `Err(String)` rather than a host trap.
fn parse_monitor_filtered(input: Value) -> Result<(Value, Pattern), Value> {
    let mut args = match input {
        Value::Tuple(args) if args.len() == 2 => args,
        _ => {
            return Err(Value::String(
                "expected (subject: string, filter: value)".into(),
            ))
        }
    };
    let filter_value = args.remove(1);
    let subject = args.remove(0);
    if !matches!(subject, Value::String(_)) {
        return Err(Value::String("expected subject actor id (string)".into()));
    }
    let filter = Pattern::try_from(filter_value)
        .map_err(|e| Value::String(format!("invalid filter pattern: {e}")))?;
    Ok((subject, filter))
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
                        .has_export("theater:simple/lifecycle-handlers", "handle-actor-event")
                        .await
                        .unwrap_or(false),
                    None => false,
                }
            };

            loop {
                tokio::select! {
                    Some((subject_id, event)) = event_rx.recv() => {
                        // The Pattern does all filtering now — a bare `monitor`
                        // watches the whole chain (match-all), `monitor-filtered`
                        // narrows it. Every delivered event is decoded and matched.
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
                                                "theater:simple/lifecycle-handlers.handle-actor-event"
                                                    .to_string(),
                                                params,
                                            )
                                            .await
                                        {
                                            error!("handle-actor-event delivery failed: {}", e);
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

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        if ctx.is_satisfied("theater:simple/lifecycle") {
            return Ok(());
        }

        let id = ctx
            .actor_id
            .ok_or_else(|| anyhow::anyhow!("actor_id not set in HandlerContext"))?;

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
        let monitor_filtered = (
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

        // link(subject) -> result<_, string>
        imports.define(
            "theater:simple/lifecycle",
            "link",
            pact_result_host_fn(move |input: Value| {
                let (theater_tx, event_tx, subs, self_id) = link.clone();
                async move {
                    add_subscription(
                        id,
                        &input,
                        &theater_tx,
                        event_tx,
                        &subs,
                        &self_id,
                        vec![any_termination()],
                        Target::StopSelf,
                    )
                }
            }),
        );
        // monitor(subject) -> result<_, string>
        // A bare monitor watches the WHOLE chain: a match-all `Pattern::any()`
        // filter, so every chain event of the subject is delivered.
        imports.define(
            "theater:simple/lifecycle",
            "monitor",
            pact_result_host_fn(move |input: Value| {
                let (theater_tx, event_tx, subs, self_id) = monitor.clone();
                async move {
                    add_subscription(
                        id,
                        &input,
                        &theater_tx,
                        event_tx,
                        &subs,
                        &self_id,
                        vec![Pattern::any()],
                        Target::DeliverToWasm,
                    )
                }
            }),
        );
        // monitor-filtered(subject, filter) -> result<_, string>
        // Like `monitor`, but the caller supplies the match filter (a serialized
        // `packr_abi::Pattern`) instead of the whole-chain `Pattern::any()`.
        imports.define(
            "theater:simple/lifecycle",
            "monitor-filtered",
            pact_result_host_fn(move |input: Value| {
                let (theater_tx, event_tx, subs, self_id) = monitor_filtered.clone();
                async move {
                    let (subject, filter) = parse_monitor_filtered(input)?;
                    add_subscription(
                        id,
                        &subject,
                        &theater_tx,
                        event_tx,
                        &subs,
                        &self_id,
                        vec![filter],
                        Target::DeliverToWasm,
                    )
                }
            }),
        );
        // unlink(subject) -> result<_, string>
        imports.define(
            "theater:simple/lifecycle",
            "unlink",
            pact_result_host_fn(move |input: Value| {
                let (theater_tx, event_tx, subs) = unlink.clone();
                async move {
                    remove_subscription(&input, &theater_tx, event_tx, &subs, Target::StopSelf)
                }
            }),
        );
        // unmonitor(subject) -> result<_, string>
        imports.define(
            "theater:simple/lifecycle",
            "unmonitor",
            pact_result_host_fn(move |input: Value| {
                let (theater_tx, event_tx, subs) = unmonitor.clone();
                async move {
                    remove_subscription(&input, &theater_tx, event_tx, &subs, Target::DeliverToWasm)
                }
            }),
        );
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
    id: TheaterId,
    input: &Value,
    theater_tx: &UnboundedSender<TheaterCommand>,
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
    subs: &Subs,
    self_id: &Arc<Mutex<Option<TheaterId>>>,
    filter: Vec<Pattern>,
    target: Target,
) -> Result<Value, Value> {
    let subject = parse_subject(input)?;
    *self_id.lock().unwrap() = Some(id);
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
    use theater::subscription::any_lifecycle_event;

    #[test]
    fn handler_name_and_interface_hashes_are_stable() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let handler = LifecycleHandler::new(tx);
        assert_eq!(handler.name(), "lifecycle");
        assert_eq!(lifecycle_interface().hash(), lifecycle_interface().hash());
    }

    #[test]
    fn monitor_filtered_decodes_subject_and_pattern() {
        // A valid serialized Pattern (`any_lifecycle_event`) round-trips back to
        // the same Pattern, and the subject is preserved as a string Value.
        let pattern = any_lifecycle_event();
        let input = Value::Tuple(vec![
            Value::String("actor-123".into()),
            Value::from(pattern.clone()),
        ]);
        let (subject, decoded) = parse_monitor_filtered(input).expect("valid filtered monitor");
        assert_eq!(subject, Value::String("actor-123".into()));
        assert_eq!(Value::from(decoded), Value::from(pattern));
    }

    #[test]
    fn monitor_filtered_rejects_bad_arity_subject_and_filter() {
        // Wrong tuple arity.
        assert!(parse_monitor_filtered(Value::Tuple(vec![Value::String("a".into())])).is_err());
        // Non-string subject.
        assert!(parse_monitor_filtered(Value::Tuple(vec![
            Value::U32(1),
            Value::from(any_termination()),
        ]))
        .is_err());
        // Filter value that is not a Pattern.
        assert!(parse_monitor_filtered(Value::Tuple(vec![
            Value::String("a".into()),
            Value::U8(7),
        ]))
        .is_err());
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
