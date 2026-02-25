//! # Timer Handler
//!
//! Provides periodic tick callbacks for WebAssembly actors in the Theater system.
//! Useful for game loops, polling, heartbeats, and scheduled tasks.

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

/// Command to control timers
enum TimerCommand {
    SetInterval {
        name: String,
        interval_ms: u64,
    },
    ClearInterval {
        name: String,
    },
}

/// Shared timer state
#[derive(Clone)]
struct TimerState {
    /// Channel to send commands to the timer loop
    command_tx: mpsc::Sender<TimerCommand>,
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

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Timer handler starting");

        let config = self.config.clone();
        let state = self.state.clone();

        Box::pin(async move {
            let _ = state; // Suppress unused warning for now

            // Active timers: name -> cancel sender
            let active_timers: Arc<Mutex<HashMap<String, mpsc::Sender<()>>>> =
                Arc::new(Mutex::new(HashMap::new()));

            // If there's a default interval configured, start it
            if let Some(interval_ms) = config.interval_ms {
                info!("Starting default timer with {}ms interval", interval_ms);

                let actor = actor_handle.clone();
                let timers = active_timers.clone();
                let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

                {
                    let mut timers_guard = timers.lock().await;
                    timers_guard.insert("default".to_string(), cancel_tx);
                }

                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));

                    loop {
                        tokio::select! {
                            _ = interval.tick() => {
                                let input = Value::String("default".to_string());
                                if let Err(e) = actor
                                    .call_function(
                                        "theater:simple/timer.handle-tick".to_string(),
                                        input,
                                    )
                                    .await
                                {
                                    debug!("Timer tick call failed: {:?}", e);
                                }
                            }
                            _ = cancel_rx.recv() => {
                                info!("Default timer cancelled");
                                break;
                            }
                        }
                    }
                });
            }

            // Wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;

            // Cancel all active timers
            let timers = active_timers.lock().await;
            for (name, cancel_tx) in timers.iter() {
                debug!("Cancelling timer: {}", name);
                let _ = cancel_tx.send(()).await;
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

        // Create command channel
        let (command_tx, _command_rx) = mpsc::channel::<TimerCommand>(32);
        self.state = Some(TimerState {
            command_tx: command_tx.clone(),
        });

        let cmd_tx_interval = command_tx.clone();
        let cmd_tx_clear = command_tx.clone();

        builder
            .interface("theater:simple/timer")?
            // set-interval(name: string, interval-ms: u64) -> result<string, string>
            .func_async_result(
                "set-interval",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let tx = cmd_tx_interval.clone();
                    async move {
                        let (name, interval_ms) = parse_set_interval(&input)?;

                        // Send command to timer loop
                        tx.send(TimerCommand::SetInterval {
                            name: name.clone(),
                            interval_ms,
                        })
                        .await
                        .map_err(|e| Value::String(format!("Failed to set interval: {}", e)))?;

                        Ok::<Value, Value>(Value::String(name))
                    }
                },
            )?
            // clear-interval(name: string) -> result<_, string>
            .func_async_result(
                "clear-interval",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let tx = cmd_tx_clear.clone();
                    async move {
                        let name = parse_string(&input)?;

                        tx.send(TimerCommand::ClearInterval { name })
                            .await
                            .map_err(|e| Value::String(format!("Failed to clear interval: {}", e)))?;

                        Ok::<Value, Value>(Value::Tuple(vec![]))
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
