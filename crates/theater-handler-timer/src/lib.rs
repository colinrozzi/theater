//! # Timer Handler
//!
//! Provides periodic tick callbacks for WebAssembly actors in the Theater system.
//! Useful for game loops, polling, heartbeats, and scheduled tasks.
//!
//! This handler is passive - actors call `set-interval` to start timers.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::{HandlerConfig, TimerHandlerConfig};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded timer.pact file content
const TIMER_PACT: &str = include_str!("../../../pact/timer.pact");

/// Declare the theater:simple/timer interface from the pact file.
fn timer_interface() -> InterfaceImpl {
    let pact = parse_pact(TIMER_PACT).expect("embedded timer.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Timer State
// ============================================================================

/// Shared timer state for managing active timers
#[derive(Clone)]
struct TimerState {
    /// Active timers: name -> cancel sender
    active_timers: Arc<Mutex<HashMap<String, mpsc::Sender<()>>>>,
    /// Actor handle for calling tick functions
    actor_handle: Arc<std::sync::Mutex<Option<ActorHandle>>>,
}

impl TimerState {
    fn new() -> Self {
        Self {
            active_timers: Arc::new(Mutex::new(HashMap::new())),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

// ============================================================================
// Handler Implementation
// ============================================================================

/// Handler for providing periodic tick callbacks to WebAssembly actors.
#[derive(Clone)]
pub struct TimerHandler {
    config: TimerHandlerConfig,
    state: Option<TimerState>,
}

impl TimerHandler {
    pub fn new(config: TimerHandlerConfig) -> Self {
        Self {
            config,
            state: None,
        }
    }
}

impl Handler for TimerHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let timer_config = match config {
            Some(HandlerConfig::Timer { config }) => config.clone(),
            _ => self.config.clone(),
        };
        Box::new(TimerHandler::new(timer_config))
    }

    fn name(&self) -> &str {
        "timer"
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
        Some(vec!["theater:simple/timer".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![timer_interface()]
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Timer handler setup (passive mode)");

        // Store actor handle for use by set-interval
        if let Some(ref state) = self.state {
            let mut handle_guard = state.actor_handle.lock().unwrap();
            *handle_guard = Some(actor_handle);
        }

        let state = self.state.clone();

        // Handler is passive - actors call set-interval() to start timers
        Box::pin(async move {
            // Wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;

            // Cancel all active timers
            if let Some(ref s) = state {
                let timers = s.active_timers.lock().await;
                for (name, cancel_tx) in timers.iter() {
                    debug!("Cancelling timer: {}", name);
                    let _ = cancel_tx.send(()).await;
                }
            }

            info!("Timer handler shutting down");
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up timer host functions");

        if ctx.is_satisfied("theater:simple/timer") {
            info!("theater:simple/timer already satisfied, skipping");
            return Ok(());
        }

        // Create shared state
        let state = TimerState::new();
        self.state = Some(state.clone());

        let state_interval = state.clone();
        let state_clear = state.clone();

        builder
            .interface("theater:simple/timer")?
            // set-interval(name: string, interval-ms: u64) -> result<string, string>
            .func_async_result(
                "set-interval",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let state = state_interval.clone();
                    async move {
                        let (name, interval_ms) = parse_set_interval(&input)?;

                        // Get actor handle
                        let actor_handle = {
                            let guard = state.actor_handle.lock().unwrap();
                            guard.clone().ok_or_else(|| {
                                Value::String("Actor handle not available".to_string())
                            })?
                        };

                        // Check if timer already exists
                        {
                            let timers = state.active_timers.lock().await;
                            if timers.contains_key(&name) {
                                return Err(Value::String(format!(
                                    "Timer '{}' already exists",
                                    name
                                )));
                            }
                        }

                        // Create cancel channel
                        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

                        // Store timer
                        {
                            let mut timers = state.active_timers.lock().await;
                            timers.insert(name.clone(), cancel_tx);
                        }

                        // Spawn timer task
                        let timer_name = name.clone();
                        let timers = state.active_timers.clone();
                        tokio::spawn(async move {
                            let mut interval =
                                tokio::time::interval(Duration::from_millis(interval_ms));

                            loop {
                                tokio::select! {
                                    _ = interval.tick() => {
                                        let input = Value::String(timer_name.clone());
                                        if let Err(e) = actor_handle
                                            .call_function(
                                                "theater:simple/timer.handle-tick".to_string(),
                                                input,
                                            )
                                            .await
                                        {
                                            debug!("Timer tick call failed: {:?}", e);
                                            // Remove timer on error
                                            let mut timers_guard = timers.lock().await;
                                            timers_guard.remove(&timer_name);
                                            break;
                                        }
                                    }
                                    _ = cancel_rx.recv() => {
                                        info!("Timer '{}' cancelled", timer_name);
                                        break;
                                    }
                                }
                            }
                        });

                        info!("Timer '{}' started with {}ms interval", name, interval_ms);
                        Ok::<Value, Value>(Value::String(name))
                    }
                },
            )?
            // clear-interval(name: string) -> result<_, string>
            .func_async_result(
                "clear-interval",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let state = state_clear.clone();
                    async move {
                        let name = parse_string(&input)?;

                        // Find and cancel the timer
                        let cancel_tx = {
                            let mut timers = state.active_timers.lock().await;
                            timers.remove(&name)
                        };

                        if let Some(tx) = cancel_tx {
                            let _ = tx.send(()).await;
                            info!("Timer '{}' cleared", name);
                            Ok::<Value, Value>(Value::Tuple(vec![]))
                        } else {
                            Err(Value::String(format!("Timer '{}' not found", name)))
                        }
                    }
                },
            )?
            // now() -> u64
            .func_async_result(
                "now",
                move |_ctx: AsyncCtx<ActorStore>, _input: Value| async move {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    Ok::<Value, Value>(Value::U64(now))
                },
            )?;

        ctx.mark_satisfied("theater:simple/timer");
        info!("Timer host functions registered");

        Ok(())
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_set_interval(input: &Value) -> Result<(String, u64), Value> {
    match input {
        Value::Tuple(items) if items.len() >= 2 => {
            let name = match &items[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for name".to_string())),
            };
            let interval_ms = match &items[1] {
                Value::U64(n) => *n,
                _ => return Err(Value::String("Expected u64 for interval_ms".to_string())),
            };
            Ok((name, interval_ms))
        }
        _ => Err(Value::String("Expected tuple(string, u64)".to_string())),
    }
}

fn parse_string(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        _ => Err(Value::String("Expected string".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_interface_parses() {
        let iface = timer_interface();
        assert_eq!(iface.name(), "theater:simple/timer");
    }

    #[test]
    fn test_handler_name() {
        let handler = TimerHandler::new(TimerHandlerConfig::default());
        assert_eq!(handler.name(), "timer");
    }
}
