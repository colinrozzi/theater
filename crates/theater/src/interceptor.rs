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
//! ## ReplayRecordingInterceptor
//!
//! Used during replay execution. Combines replay and recording: returns previously
//! recorded output values from the expected chain (so WASM runs deterministically),
//! and also records those calls to a new chain (so hashes can be compared).

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

/// Combined interceptor that replays host function calls AND records them to a new chain.
///
/// During replay execution, this interceptor:
/// - Returns recorded output from `before_import` (skipping real host function execution)
/// - Records the call to the new chain in `after_import` (so hashes can be compared)
///
/// This is needed because during replay we must:
/// 1. Return recorded outputs so WASM runs deterministically
/// 2. Record those same calls to the new chain so we can compare hashes
pub struct ReplayRecordingInterceptor {
    /// Expected chain events (for looking up recorded host function outputs)
    expected_events: Vec<crate::chain::ChainEvent>,
    /// Position in expected events (for sequential host call lookup)
    position: Mutex<usize>,
    /// The new chain being built during replay (for recording)
    chain: Arc<RwLock<StateChain>>,
}

impl ReplayRecordingInterceptor {
    /// Create a new ReplayRecordingInterceptor.
    ///
    /// `expected_events` are the events from the original chain (used to look up
    /// recorded host function outputs). `chain` is the new chain being built
    /// during replay (used to record events for hash comparison).
    pub fn new(expected_events: Vec<crate::chain::ChainEvent>, chain: Arc<RwLock<StateChain>>) -> Self {
        Self {
            expected_events,
            position: Mutex::new(0),
            chain,
        }
    }

    /// Find the next HostFunction event at or after the current position,
    /// matching interface/function name.
    fn find_next_host_call(
        &self,
        interface: &str,
        function: &str,
    ) -> Option<HostFunctionCall> {
        let mut pos = self.position.lock().unwrap();
        while *pos < self.expected_events.len() {
            let event = &self.expected_events[*pos];

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

impl CallInterceptor for ReplayRecordingInterceptor {
    fn before_import(&self, interface: &str, function: &str, _input: &Value) -> Option<Value> {
        // Find the next matching host function call and return its recorded output
        self.find_next_host_call(interface, function)
            .map(|call| call.output)
    }

    fn after_import(&self, interface: &str, function: &str, input: &Value, output: &Value) {
        // Record to the new chain (same as RecordingInterceptor)
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
        None // Let exports execute normally during replay
    }

    fn after_export(&self, _function: &str, _input: &Value, _output: &Value) {
        // Export recording is handled by execute_call
    }
}
