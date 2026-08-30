//! # Lifecycle-Relationship Handler
//!
//! The actor-facing surface over the runtime's subscription substrate
//! ([`theater::subscription`]). An actor attaches a directed relationship to
//! another actor (the *subject*), always as itself (self-service; subscriber =
//! caller):
//!
//! - [`link`] / `unlink` — **fate-sharing**: a `StopSelf` subscription. When the
//!   subject terminates the **core runtime** stops the caller (the cascade). No
//!   wasm callback; the runtime is in the death path, so fate lives there.
//! - [`monitor`] / `unmonitor` — **watching**: the subject's chain feeds *this*
//!   handler's loop directly (a chain subscriber, via `SubscribeToActor`); the
//!   loop filters host-side and calls the caller's `handle-lifecycle-event`
//!   export. The runtime is *not* in this path — events flow chain → handler →
//!   wasm, which is where watch signals actually travel.
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
use theater::subscription::{any_lifecycle_event, any_termination, Subscription, Target};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, IntoValue, LinkerError, Pattern,
    TypeHash, Value, ValueType,
};

/// Import side: `link`/`monitor` host functions the actor calls.
const LIFECYCLE_PACT: &str = include_str!("../lifecycle.pact");
/// Export side: the `handle-lifecycle-event` callback the actor implements. The
/// canonical contract source; exports are matched by name (`has_export`), so
/// this is consumed by the test + downstream actors rather than the handler.
#[allow(dead_code)]
const LIFECYCLE_HANDLERS_PACT: &str = include_str!("../lifecycle-handlers.pact");

/// Alias for the monitor delivery receiver, taken by `setup` once.
type MonitorEventRx = Arc<Mutex<Option<mpsc::Receiver<(TheaterId, ChainEvent)>>>>;

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
/// Per-actor state (the monitor event channel + filters) is **fresh per
/// instance** — `create_instance` builds a new one rather than cloning shared
/// `Arc`s, so each actor's monitors are its own (cf. the handler-clone-shares-
/// state trap).
pub struct LifecycleHandler {
    theater_tx: UnboundedSender<TheaterCommand>,
    /// Chain-subscriber sender handed to the subjects this actor monitors;
    /// their events arrive on `event_rx` (drained by `setup`).
    event_tx: mpsc::Sender<(TheaterId, ChainEvent)>,
    event_rx: MonitorEventRx,
    /// Per-subject filters this actor monitors with — the host-side match run
    /// before waking the actor's `handle-lifecycle-event` export.
    filters: Arc<Mutex<HashMap<TheaterId, Vec<Pattern>>>>,
}

impl LifecycleHandler {
    pub fn new(theater_tx: UnboundedSender<TheaterCommand>) -> Self {
        Self::fresh(theater_tx)
    }

    /// Build a genuinely independent instance: a fresh monitor event channel and
    /// empty filter set. Only the process-global command channel carries over.
    fn fresh(theater_tx: UnboundedSender<TheaterCommand>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            theater_tx,
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            filters: Arc::new(Mutex::new(HashMap::new())),
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
        let filters = self.filters.clone();

        Box::pin(async move {
            // A cloned instance (no receiver) just waits for shutdown.
            let Some(mut event_rx) = event_rx_opt else {
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            };

            // Only actors that implement the callback get monitor deliveries.
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
                        if !has_callback {
                            continue;
                        }
                        // Cheap pre-filter, then the real structural match — both
                        // host-side, so a non-matching event never wakes wasm.
                        if !LIFECYCLE_EVENT_TYPES.contains(&event.event_type.as_str()) {
                            continue;
                        }
                        let matched = {
                            let f = filters.lock().unwrap();
                            f.get(&subject_id).is_some_and(|pats| {
                                decode_chain_event_payload(&event.data)
                                    .map(|payload| {
                                        let v = payload.into_value();
                                        pats.iter().any(|p| p.matches(&v))
                                    })
                                    .unwrap_or(false)
                            })
                        };
                        if matched {
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

        // link/unlink -> the StopSelf fate set (runtime map + cascade).
        let link_tx = self.theater_tx.clone();
        let unlink_tx = self.theater_tx.clone();
        // monitor/unmonitor -> a chain subscriber feeding this handler's loop.
        let monitor_tx = self.theater_tx.clone();
        let monitor_event_tx = self.event_tx.clone();
        let monitor_filters = self.filters.clone();
        let unmonitor_tx = self.theater_tx.clone();
        let unmonitor_event_tx = self.event_tx.clone();
        let unmonitor_filters = self.filters.clone();

        builder
            .interface("theater:simple/lifecycle")?
            // link(subject) -> result<_, string>
            .func_async_result("link", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let theater_tx = link_tx.clone();
                async move {
                    let subject = parse_subject(&input)?;
                    let caller = ctx.data().id;
                    let _ = theater_tx.send(TheaterCommand::Subscribe {
                        subject,
                        subscription: Subscription {
                            subscriber: caller,
                            filter: vec![any_termination()],
                            target: Target::StopSelf,
                        },
                    });
                    Ok::<Value, Value>(Value::Tuple(vec![]))
                }
            })?
            // unlink(subject) -> result<_, string>
            .func_async_result("unlink", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let theater_tx = unlink_tx.clone();
                async move {
                    let subject = parse_subject(&input)?;
                    let caller = ctx.data().id;
                    let _ = theater_tx.send(TheaterCommand::Unsubscribe {
                        subject,
                        subscriber: caller,
                    });
                    Ok::<Value, Value>(Value::Tuple(vec![]))
                }
            })?
            // monitor(subject) -> result<_, string>
            .func_async_result(
                "monitor",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = monitor_tx.clone();
                    let event_tx = monitor_event_tx.clone();
                    let filters = monitor_filters.clone();
                    async move {
                        let subject = parse_subject(&input)?;
                        filters
                            .lock()
                            .unwrap()
                            .insert(subject, vec![any_lifecycle_event()]);
                        // Route the subject's chain events to this handler's loop.
                        let _ = theater_tx.send(TheaterCommand::SubscribeToActor {
                            actor_id: subject,
                            event_tx,
                        });
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // unmonitor(subject) -> result<_, string>
            .func_async_result(
                "unmonitor",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = unmonitor_tx.clone();
                    let event_tx = unmonitor_event_tx.clone();
                    let filters = unmonitor_filters.clone();
                    async move {
                        let subject = parse_subject(&input)?;
                        filters.lock().unwrap().remove(&subject);
                        let _ = theater_tx.send(TheaterCommand::UnsubscribeFromActor {
                            actor_id: subject,
                            event_tx,
                        });
                        Ok::<Value, Value>(Value::Tuple(vec![]))
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
        // The monitor callback the actor optionally implements.
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
