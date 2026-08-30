//! # Lifecycle-Relationship Handler
//!
//! The actor-facing surface over the runtime's subscription substrate
//! ([`theater::subscription`]). It lets an actor attach a directed relationship
//! to another actor (the *subject*) — always as itself (self-service; the
//! runtime records `subscriber = caller`):
//!
//! - [`link`] — fate-sharing: when the subject terminates the runtime stops the
//!   caller (a `StopSelf` subscription, enacted by the cascade; no wasm call).
//! - [`monitor`] — watching: matching subject events are delivered to the
//!   caller's `handle-lifecycle-event` export (a `DeliverToWasm` subscription).
//!
//! v1 uses fixed filters (link → any termination, monitor → any lifecycle
//! event); custom structural (`packr_abi::Pattern`) filters are a forward
//! addition once the pattern crosses the wasm↔host boundary as a pact type.
//!
//! `link`/`unlink` are fully enacted by the runtime cascade today. `monitor`
//! registers the subscription; delivery of events to `handle-lifecycle-event`
//! is wired in the runtime's `DeliverToWasm` dispatch.

use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use tracing::info;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::id::TheaterId;
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;
use theater::subscription::{any_lifecycle_event, any_termination, Subscription, Target};
use tokio::sync::mpsc::UnboundedSender;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
};

/// Embedded `lifecycle.pact` interface definition.
const LIFECYCLE_PACT: &str = include_str!("../lifecycle.pact");

fn lifecycle_interface() -> InterfaceImpl {
    let pact = parse_pact(LIFECYCLE_PACT).expect("embedded lifecycle.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// Handler providing `theater:simple/lifecycle` (link / monitor) to actors.
#[derive(Clone)]
pub struct LifecycleHandler {
    theater_tx: UnboundedSender<TheaterCommand>,
}

impl LifecycleHandler {
    pub fn new(theater_tx: UnboundedSender<TheaterCommand>) -> Self {
        Self { theater_tx }
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
        Box::new(self.clone())
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        Box::pin(async {
            shutdown_receiver.wait_for_shutdown().await;
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

        // A subscription-registering op: resolve caller + subject, send a
        // `Subscribe` with the given target/filter. The send is on the unbounded
        // command channel, so it never blocks.
        let subscribe_tx = self.theater_tx.clone();
        let unsubscribe_tx = self.theater_tx.clone();
        let monitor_tx = self.theater_tx.clone();
        let unmonitor_tx = self.theater_tx.clone();

        builder
            .interface("theater:simple/lifecycle")?
            // link(subject) -> result<_, string>
            .func_async_result("link", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let theater_tx = subscribe_tx.clone();
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
                let theater_tx = unsubscribe_tx.clone();
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
            .func_async_result("monitor", move |ctx: AsyncCtx<ActorStore>, input: Value| {
                let theater_tx = monitor_tx.clone();
                async move {
                    let subject = parse_subject(&input)?;
                    let caller = ctx.data().id;
                    let _ = theater_tx.send(TheaterCommand::Subscribe {
                        subject,
                        subscription: Subscription {
                            subscriber: caller,
                            filter: vec![any_lifecycle_event()],
                            target: Target::DeliverToWasm,
                        },
                    });
                    Ok::<Value, Value>(Value::Tuple(vec![]))
                }
            })?
            // unmonitor(subject) -> result<_, string>
            .func_async_result(
                "unmonitor",
                move |ctx: AsyncCtx<ActorStore>, input: Value| {
                    let theater_tx = unmonitor_tx.clone();
                    async move {
                        let subject = parse_subject(&input)?;
                        let caller = ctx.data().id;
                        let _ = theater_tx.send(TheaterCommand::Unsubscribe {
                            subject,
                            subscriber: caller,
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
        // link/unlink require no wasm callback. The `handle-lifecycle-event`
        // export lands with monitor's DeliverToWasm delivery.
        None
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
    use tokio::sync::mpsc;

    #[test]
    fn handler_name_and_interface_hash_are_stable() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let handler = LifecycleHandler::new(tx);
        assert_eq!(handler.name(), "lifecycle");
        let h1 = lifecycle_interface().hash();
        let h2 = lifecycle_interface().hash();
        assert_eq!(h1, h2, "interface hash must be deterministic");
    }

    #[test]
    fn interface_is_theater_simple_lifecycle() {
        let iface = lifecycle_interface();
        assert_eq!(iface.name(), "theater:simple/lifecycle");
    }
}
