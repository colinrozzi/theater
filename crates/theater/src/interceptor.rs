//! # Call Interceptors
//!
//! This module provides implementations of Pack's `CallInterceptor` trait for
//! automatic recording and replay of host function calls.
//!
//! ## RecordingInterceptor
//!
//! Used during normal execution. Allows all calls to proceed normally and records
//! input/output to the actor's chain after each call completes.
//!
//! ## ReplayInterceptor
//!
//! Used during replay execution. Short-circuits host function calls by returning
//! previously recorded output values from the chain, without executing the real
//! host function.

use std::sync::{Arc, Mutex, RwLock};

use pack::abi::Value;
use pack::CallInterceptor;

use crate::chain::StateChain;
use crate::events::{ChainEventData, ChainEventPayload};
use crate::replay::HostFunctionCall;

/// Interceptor that records all host function calls to the actor's chain.
///
/// During normal execution, this interceptor:
/// - Returns `None` from `before_import` (allowing real execution)
/// - Records the input/output in `after_import` to the chain
pub struct RecordingInterceptor {
    chain: Arc<RwLock<StateChain>>,
}

impl RecordingInterceptor {
    /// Create a new RecordingInterceptor that records to the given chain.
    pub fn new(chain: Arc<RwLock<StateChain>>) -> Self {
        Self { chain }
    }
}

impl CallInterceptor for RecordingInterceptor {
    fn before_import(&self, _interface: &str, _function: &str, _input: &Value) -> Option<Value> {
        None // Always proceed with real execution
    }

    fn after_import(&self, interface: &str, function: &str, input: &Value, output: &Value) {
        let call = HostFunctionCall::new(
            interface,
            function,
            input.clone(),
            output.clone(),
        );

        let mut chain = self.chain.write().unwrap();
        let _ = chain.add_typed_event(ChainEventData {
            event_type: format!("{}/{}", interface, function),
            data: ChainEventPayload::HostFunction(call),
        });
    }

    fn before_export(&self, _function: &str, _input: &Value) -> Option<Value> {
        None // Always proceed with real execution
    }

    fn after_export(&self, _function: &str, _input: &Value, _output: &Value) {
        // Export calls are already recorded by the actor runtime as WasmCall/WasmResult events
    }
}

/// Interceptor that replays host function calls from a recorded chain.
///
/// During replay execution, this interceptor:
/// - Returns recorded output from `before_import` (skipping real execution)
/// - Advances through the chain events sequentially
pub struct ReplayInterceptor {
    /// The chain events containing recorded host function calls
    events: Vec<crate::chain::ChainEvent>,
    /// Current position in the events list
    position: Mutex<usize>,
}

impl ReplayInterceptor {
    /// Create a new ReplayInterceptor from recorded chain events.
    ///
    /// Only HostFunction events are used; other event types are skipped
    /// during replay.
    pub fn new(events: Vec<crate::chain::ChainEvent>) -> Self {
        Self {
            events,
            position: Mutex::new(0),
        }
    }

    /// Get current replay position.
    pub fn position(&self) -> usize {
        *self.position.lock().unwrap()
    }

    /// Get total number of events.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Check if replay is complete.
    pub fn is_complete(&self) -> bool {
        self.position() >= self.events.len()
    }

    /// Find the next HostFunction event at or after the current position,
    /// optionally matching interface/function name.
    fn find_next_host_call(
        &self,
        interface: &str,
        function: &str,
    ) -> Option<HostFunctionCall> {
        let mut pos = self.position.lock().unwrap();
        while *pos < self.events.len() {
            let event = &self.events[*pos];

            // Try to deserialize the event data as a ChainEventPayload
            if let Ok(payload) = serde_json::from_slice::<ChainEventPayload>(&event.data) {
                if let ChainEventPayload::HostFunction(call) = payload {
                    if call.interface == interface && call.function == function {
                        *pos += 1;
                        return Some(call);
                    }
                }
            }

            // Also try deserializing directly as HostFunctionCall (backward compat)
            if let Ok(call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
                if call.interface == interface && call.function == function {
                    *pos += 1;
                    return Some(call);
                }
            }

            // Skip non-matching events (e.g., Wasm events)
            *pos += 1;
        }
        None
    }
}

impl CallInterceptor for ReplayInterceptor {
    fn before_import(&self, interface: &str, function: &str, _input: &Value) -> Option<Value> {
        // Find the next matching host function call and return its recorded output
        self.find_next_host_call(interface, function)
            .map(|call| call.output)
    }

    fn after_import(&self, _interface: &str, _function: &str, _input: &Value, _output: &Value) {
        // Nothing to do during replay - the call was already handled by before_import
    }

    fn before_export(&self, _function: &str, _input: &Value) -> Option<Value> {
        None // Let exports execute normally during replay
    }

    fn after_export(&self, _function: &str, _input: &Value, _output: &Value) {
        // Nothing to do during replay
    }
}
