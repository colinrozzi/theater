//! # Loop Handler
//!
//! Provides cooperative looping for WebAssembly actors in the Theater system.
//!
//! Instead of blocking in a tight loop, actors yield after each iteration,
//! allowing the runtime to:
//! - Record state transitions to the chain
//! - Process other messages (RPC, timers, etc.)
//! - Schedule other actors fairly
//!
//! ## Usage
//!
//! Actor imports `theater:simple/loop` and exports `theater:simple/loop-client`:
//!
//! ```ignore
//! // Actor calls start-loop with initial state
//! loop::start_loop(serialize(&initial_state))?;
//!
//! // Runtime repeatedly calls this export
//! #[export]
//! fn loop(state: Vec<u8>) -> Result<Vec<u8>, String> {
//!     let mut state: MyState = deserialize(&state)?;
//!     state.tick();
//!     Ok(serialize(&state))
//! }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;
use tracing::{debug, error, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;

use theater::pack_bridge::{
    parse_pact, AsyncCtx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
    ValueType,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded loop.pact file content
const LOOP_PACT: &str = include_str!("../../../pact/loop.pact");

/// Declare the theater:simple/loop interface from the pact file.
fn loop_interface() -> InterfaceImpl {
    let pact = parse_pact(LOOP_PACT).expect("embedded loop.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

// ============================================================================
// Loop State
// ============================================================================

/// Shared state for the loop handler
struct LoopState {
    /// Whether the loop is currently running
    running: AtomicBool,
}

impl LoopState {
    fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn start(&self) -> bool {
        // Returns true if we successfully started (wasn't already running)
        !self.running.swap(true, Ordering::SeqCst)
    }

    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

// ============================================================================
// Handler Implementation
// ============================================================================

/// Handler for providing cooperative looping to WebAssembly actors.
#[derive(Clone)]
pub struct LoopHandler {
    /// Loop state (shared with background task)
    state: Arc<LoopState>,
    /// Actor handle for calling the loop export
    actor_handle: Arc<std::sync::Mutex<Option<ActorHandle>>>,
    /// Shutdown receiver for the loop task (used by start-loop())
    shutdown_receiver: Arc<std::sync::Mutex<Option<ShutdownReceiver>>>,
    /// Shutdown receiver for setup() itself - ensures setup() can exit on shutdown
    setup_shutdown_receiver: Arc<std::sync::Mutex<Option<ShutdownReceiver>>>,
    /// Notifies setup() when start-loop() takes the shutdown receiver
    loop_started_notify: Arc<Notify>,
}

impl Default for LoopHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopHandler {
    pub fn new() -> Self {
        Self {
            state: Arc::new(LoopState::new()),
            actor_handle: Arc::new(std::sync::Mutex::new(None)),
            shutdown_receiver: Arc::new(std::sync::Mutex::new(None)),
            setup_shutdown_receiver: Arc::new(std::sync::Mutex::new(None)),
            loop_started_notify: Arc::new(Notify::new()),
        }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![loop_interface()]
    }
}

impl Handler for LoopHandler {
    fn create_instance(
        &self,
        _config: Option<&theater::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(LoopHandler::new())
    }

    fn name(&self) -> &str {
        "loop"
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
        Some(vec!["theater:simple/loop-client".to_string()])
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![loop_interface()]
    }

    fn setup(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: tokio::sync::broadcast::Receiver<theater::chain::ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Loop handler setup");

        // Store handles for use by start-loop
        {
            let mut handle_guard = self.actor_handle.lock().unwrap();
            *handle_guard = Some(actor_handle);
        }

        // Try to get setup_shutdown_receiver from setup_host_functions_composite
        // If not available, use the passed shutdown_receiver directly for setup
        let setup_receiver = {
            let mut guard = self.setup_shutdown_receiver.lock().unwrap();
            guard.take()
        };

        let loop_started_notify = self.loop_started_notify.clone();

        // Use either the setup receiver (from setup_host_functions_composite) or
        // the passed shutdown_receiver directly for setup
        let mut receiver_for_setup = match setup_receiver {
            Some(r) => {
                // We have a dedicated setup receiver, so store the passed one for start-loop
                let mut shutdown_guard = self.shutdown_receiver.lock().unwrap();
                if shutdown_guard.is_none() {
                    *shutdown_guard = Some(shutdown_receiver);
                }
                r
            }
            None => {
                // No setup receiver available, use the passed one directly
                shutdown_receiver
            }
        };

        // Wait for either:
        // 1. start-loop() is called (notified via loop_started_notify)
        // 2. Shutdown signal received (if start-loop() was never called)
        Box::pin(async move {
            tokio::select! {
                _ = loop_started_notify.notified() => {
                    info!("Loop handler: loop started, setup complete");
                }
                _ = &mut receiver_for_setup.receiver => {
                    info!("Loop handler: shutdown before start-loop, exiting setup");
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
        info!("Setting up Loop host functions");

        if ctx.is_satisfied("theater:simple/loop") {
            info!("theater:simple/loop already satisfied, skipping");
            return Ok(());
        }

        // Subscribe to two shutdown receivers:
        // 1. One for start-loop() to use in the loop task
        // 2. One for setup() to use so it can exit if start-loop() is never called
        if let Some(shutdown_receiver) = ctx.subscribe_shutdown() {
            let mut guard = self.shutdown_receiver.lock().unwrap();
            *guard = Some(shutdown_receiver);
        }
        if let Some(setup_shutdown_receiver) = ctx.subscribe_shutdown() {
            let mut guard = self.setup_shutdown_receiver.lock().unwrap();
            *guard = Some(setup_shutdown_receiver);
        }

        let state = self.state.clone();
        let actor_handle_arc = self.actor_handle.clone();
        let shutdown_receiver_arc = self.shutdown_receiver.clone();
        let loop_started_notify = self.loop_started_notify.clone();

        let state_for_stop = state.clone();

        builder
            .interface("theater:simple/loop")?
            // ----------------------------------------------------------------
            // start-loop(initial-state: list<u8>) -> result<_, string>
            // ----------------------------------------------------------------
            .func_async_result(
                "start-loop",
                move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                    let state = state.clone();
                    let actor_handle_arc = actor_handle_arc.clone();
                    let shutdown_receiver_arc = shutdown_receiver_arc.clone();
                    let loop_started_notify = loop_started_notify.clone();

                    async move {
                        // Parse initial state
                        let initial_state = match &input {
                            Value::List { items, .. } => items
                                .iter()
                                .filter_map(|v| match v {
                                    Value::U8(b) => Some(*b),
                                    _ => None,
                                })
                                .collect::<Vec<u8>>(),
                            Value::Tuple(items) if items.len() == 1 => match &items[0] {
                                Value::List { items, .. } => items
                                    .iter()
                                    .filter_map(|v| match v {
                                        Value::U8(b) => Some(*b),
                                        _ => None,
                                    })
                                    .collect::<Vec<u8>>(),
                                _ => {
                                    return Err(Value::String(
                                        "Expected list<u8> for initial state".to_string(),
                                    ))
                                }
                            },
                            _ => {
                                return Err(Value::String(
                                    "Expected list<u8> for initial state".to_string(),
                                ))
                            }
                        };

                        // Try to start the loop
                        if !state.start() {
                            return Err(Value::String("Loop is already running".to_string()));
                        }

                        info!("Starting cooperative loop with {} bytes of state", initial_state.len());

                        // Get actor handle
                        let actor_handle = {
                            let guard = actor_handle_arc.lock().unwrap();
                            guard.clone()
                        };
                        let Some(actor_handle) = actor_handle else {
                            state.stop();
                            return Err(Value::String(
                                "Actor handle not available - setup() not called?".to_string(),
                            ));
                        };

                        // Get shutdown receiver (if available)
                        let shutdown_receiver = {
                            let mut guard = shutdown_receiver_arc.lock().unwrap();
                            guard.take()
                        };

                        // Notify setup() that we've taken the receiver
                        loop_started_notify.notify_one();

                        // Spawn the loop task
                        let state_for_task = state.clone();
                        tokio::spawn(async move {
                            let mut current_state = initial_state;
                            let mut iteration = 0u64;

                            // The main loop - runs until stopped, shutdown, or error
                            let loop_future = async {
                                loop {
                                    // Check if we should stop
                                    if !state_for_task.is_running() {
                                        info!("Loop stopped by stop-loop after {} iterations", iteration);
                                        break;
                                    }

                                    // Build params for loop call
                                    let params = Value::List {
                                        elem_type: ValueType::U8,
                                        items: current_state.iter().map(|&b| Value::U8(b)).collect(),
                                    };

                                    // Call the actor's loop export
                                    let result = actor_handle
                                        .call_function(
                                            "theater:simple/loop-client.loop".to_string(),
                                            params,
                                        )
                                        .await;

                                    match result {
                                        Ok(return_value) => {
                                            // Parse the result - should be result<list<u8>, string>
                                            match extract_loop_result(&return_value) {
                                                Ok(new_state) => {
                                                    current_state = new_state;
                                                    iteration += 1;

                                                    if iteration % 10000 == 0 {
                                                        debug!("Loop iteration {}", iteration);
                                                    }

                                                    // Yield to allow other tasks to run
                                                    tokio::task::yield_now().await;
                                                }
                                                Err(err_msg) => {
                                                    info!(
                                                        "Loop stopped by actor error after {} iterations: {}",
                                                        iteration, err_msg
                                                    );
                                                    state_for_task.stop();
                                                    break;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "Loop stopped by runtime error after {} iterations: {:?}",
                                                iteration, e
                                            );
                                            state_for_task.stop();
                                            break;
                                        }
                                    }
                                }
                            };

                            // Run the loop, with optional shutdown handling
                            if let Some(shutdown_rx) = shutdown_receiver {
                                tokio::select! {
                                    _ = shutdown_rx.wait_for_shutdown() => {
                                        info!("Loop stopped by shutdown after {} iterations", iteration);
                                        state_for_task.stop();
                                    }
                                    _ = loop_future => {}
                                }
                            } else {
                                loop_future.await;
                            }

                            info!("Loop task exited after {} iterations", iteration);
                        });

                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?
            // ----------------------------------------------------------------
            // stop-loop() -> result<_, string>
            // ----------------------------------------------------------------
            .func_async_result(
                "stop-loop",
                move |_ctx: AsyncCtx<ActorStore>, _input: Value| {
                    let state = state_for_stop.clone();
                    async move {
                        if !state.is_running() {
                            return Err(Value::String("Loop is not running".to_string()));
                        }

                        state.stop();
                        info!("Loop stop requested");
                        Ok::<Value, Value>(Value::Tuple(vec![]))
                    }
                },
            )?;

        ctx.mark_satisfied("theater:simple/loop");
        info!("Loop host functions set up successfully");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

/// Extract the result from a loop return value.
/// Expected format: result<list<u8>, string> which is a Variant.
fn extract_loop_result(value: &Value) -> Result<Vec<u8>, String> {
    match value {
        Value::Variant {
            case_name,
            payload,
            ..
        } => {
            if case_name == "ok" {
                // Extract the list<u8> from the ok payload
                if let Some(inner) = payload.first() {
                    match inner {
                        Value::List { items, .. } => {
                            let bytes: Vec<u8> = items
                                .iter()
                                .filter_map(|v| match v {
                                    Value::U8(b) => Some(*b),
                                    _ => None,
                                })
                                .collect();
                            Ok(bytes)
                        }
                        _ => Err("Expected list<u8> in ok variant".to_string()),
                    }
                } else {
                    Err("Empty ok payload".to_string())
                }
            } else if case_name == "err" {
                // Extract the error string
                if let Some(Value::String(msg)) = payload.first() {
                    Err(msg.clone())
                } else {
                    Err("Unknown error".to_string())
                }
            } else {
                Err(format!("Unexpected variant case: {}", case_name))
            }
        }
        _ => Err(format!("Expected result variant, got {:?}", value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_handler_creation() {
        let handler = LoopHandler::new();
        assert_eq!(handler.name(), "loop");
        assert_eq!(
            handler.imports(),
            Some(vec!["theater:simple/loop".to_string()])
        );
        assert_eq!(
            handler.exports(),
            Some(vec!["theater:simple/loop-client".to_string()])
        );
    }

    #[test]
    fn test_loop_interface_hash_determinism() {
        let interface1 = loop_interface();
        let interface2 = loop_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_loop_state() {
        let state = LoopState::new();
        assert!(!state.is_running());

        assert!(state.start()); // First start succeeds
        assert!(state.is_running());

        assert!(!state.start()); // Second start fails (already running)

        state.stop();
        assert!(!state.is_running());

        assert!(state.start()); // Can start again after stop
    }

    #[test]
    fn test_extract_loop_result_ok() {
        let ok_value = Value::Variant {
            type_name: "result".to_string(),
            case_name: "ok".to_string(),
            tag: 0,
            payload: vec![Value::List {
                elem_type: ValueType::U8,
                items: vec![Value::U8(1), Value::U8(2), Value::U8(3)],
            }],
        };

        let result = extract_loop_result(&ok_value);
        assert_eq!(result, Ok(vec![1, 2, 3]));
    }

    #[test]
    fn test_extract_loop_result_err() {
        let err_value = Value::Variant {
            type_name: "result".to_string(),
            case_name: "err".to_string(),
            tag: 1,
            payload: vec![Value::String("something went wrong".to_string())],
        };

        let result = extract_loop_result(&err_value);
        assert_eq!(result, Err("something went wrong".to_string()));
    }
}
