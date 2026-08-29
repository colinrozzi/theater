//! Theater Runtime Handler
//!
//! The thin SYSTEM-level interface `theater:simple/runtime`: operate on / observe
//! the runtime as a WHOLE, not individual actors (that is supervisor's job).
//!
//! - `shutdown-runtime` — shut the whole runtime down (mutate)
//! - `subscribe-to-spawns` / `unsubscribe-from-spawns` — observe the actor
//!   population: every actor spawned anywhere is delivered to this actor's
//!   `handle-actor-spawn` export (births only; a death rides that actor's own
//!   chain subscription via supervisor.subscribe-to-actor). (inspect)
//!
//! Capability-gated by RuntimePermissions { inspect, mutate }.

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RuntimeHostConfig;
use theater::config::permissions::RuntimePermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::messages::TheaterCommand;
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value, ValueType,
};

use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use theater::id::TheaterId;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Embedded runtime.pact file content
const RUNTIME_PACT: &str = include_str!("../runtime.pact");

fn runtime_interface() -> InterfaceImpl {
    let pact = parse_pact(RUNTIME_PACT).expect("embedded runtime.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// Interface error for `theater:simple/runtime` — mirrors the `runtime-error`
/// pact variant. A normal Rust enum; the single `From<RuntimeError> for Value`
/// below is the only place a pact error value is built.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<RuntimeError> for Value {
    fn from(e: RuntimeError) -> Value {
        let (tag, case, msg) = match e {
            RuntimeError::PermissionDenied(m) => (0, "permission-denied", m),
            RuntimeError::InvalidArgument(m) => (1, "invalid-argument", m),
            RuntimeError::Internal(m) => (2, "internal", m),
        };
        Value::Variant {
            type_name: "runtime-error".to_string(),
            case_name: case.to_string(),
            tag,
            payload: vec![Value::String(msg)],
        }
    }
}

/// Enforce the runtime capability. `mutate` = shutdown-runtime; otherwise
/// inspect (subscribe-to-spawns). Default-deny when the capability is absent.
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

type SpawnEvent = (TheaterId, String, Option<TheaterId>);

/// The RuntimeHandler exposes the thin system interface to a granted actor.
///
/// Per-actor instantiation goes through [`Self::fresh`] (via `create_instance`)
/// so each actor gets its own spawn-event channel.
#[derive(Clone)]
pub struct RuntimeHandler {
    event_tx: mpsc::Sender<SpawnEvent>,
    event_rx: Arc<Mutex<Option<mpsc::Receiver<SpawnEvent>>>>,
    permissions: Option<RuntimePermissions>,
}

impl RuntimeHandler {
    pub fn new(_config: RuntimeHostConfig, permissions: Option<RuntimePermissions>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            permissions,
        }
    }

    fn fresh(&self) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            event_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
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
        info!("Setting up runtime (system) host functions (Pack)");
        if ctx.is_satisfied("theater:simple/runtime") {
            info!("theater:simple/runtime already satisfied by another handler, skipping");
            return Ok(());
        }

        let event_tx = self.event_tx.clone();
        let permissions = self.permissions.clone();

        builder
            .interface("theater:simple/runtime")?
            // shutdown-runtime: func() -> result<_, runtime-error>   (mutate)
            .func_async_result("shutdown-runtime", {
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, true)?;
                        let tx = ctx.data().theater_tx.clone();
                        if let Err(e) = tx.send(TheaterCommand::ShutdownRuntime).await {
                            return Err(Value::from(RuntimeError::Internal(format!(
                                "failed to send shutdown: {}",
                                e
                            ))));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            })?
            // subscribe-to-spawns: func() -> result<_, runtime-error>   (inspect)
            .func_async_result("subscribe-to-spawns", {
                let event_tx = event_tx.clone();
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let event_tx = event_tx.clone();
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let tx = ctx.data().theater_tx.clone();
                        if let Err(e) = tx
                            .send(TheaterCommand::SubscribeToSpawns { event_tx })
                            .await
                        {
                            return Err(Value::from(RuntimeError::Internal(format!(
                                "failed to send subscribe: {}",
                                e
                            ))));
                        }
                        Ok(Value::Tuple(vec![]))
                    }
                }
            })?
            // unsubscribe-from-spawns: func() -> result<_, runtime-error>
            .func_async_result("unsubscribe-from-spawns", {
                let event_tx = event_tx.clone();
                let permissions = permissions.clone();
                move |ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let event_tx = event_tx.clone();
                    let permissions = permissions.clone();
                    async move {
                        require(&permissions, false)?;
                        let tx = ctx.data().theater_tx.clone();
                        if let Err(e) = tx
                            .send(TheaterCommand::UnsubscribeFromSpawns { event_tx })
                            .await
                        {
                            return Err(Value::from(RuntimeError::Internal(format!(
                                "failed to send unsubscribe: {}",
                                e
                            ))));
                        }
                        Ok(Value::Tuple(vec![]))
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
        info!("Runtime (system) handler setup");
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
