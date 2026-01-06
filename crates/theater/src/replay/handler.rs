//! # Replay Handler
//!
//! The ReplayHandler is a special handler that replays actors from recorded event chains.
//! When registered, it dynamically discovers all component imports and registers stub
//! functions that return the recorded outputs.
//!
//! ## Usage
//!
//! ```ignore
//! // Load the expected chain from a previous run
//! let expected_chain = load_chain("actor_chain.json")?;
//!
//! // Create a handler registry with just the replay handler
//! let mut registry = HandlerRegistry::new();
//! registry.register(ReplayHandler::new(expected_chain));
//!
//! // Run the actor - it will replay using recorded outputs
//! let runtime = TheaterRuntime::new(..., registry);
//! ```

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};
use wasmtime::component::types::ComponentItem;
use wasmtime::component::Val;
use wasmtime::StoreContextMut;

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::chain::ChainEvent;
use crate::events::{ChainEventData, ChainEventPayload};
use crate::handler::{Handler, HandlerContext, SharedActorInstance};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};

use super::HostFunctionCall;

/// Shared state for tracking replay position across all stub functions.
#[derive(Clone)]
pub struct ReplayState {
    /// The expected chain events
    events: Arc<Vec<ChainEvent>>,
    /// Current position in the chain
    position: Arc<Mutex<usize>>,
    /// List of interfaces discovered from the chain
    interfaces: Arc<Vec<String>>,
}

impl ReplayState {
    /// Create a new ReplayState from a chain of events.
    pub fn new(events: Vec<ChainEvent>) -> Self {
        // Extract unique interfaces from the chain events
        let interfaces: Vec<String> = events
            .iter()
            .filter_map(|event| {
                // Parse event_type to extract interface
                // Format: "interface/function" (e.g., "theater:simple/runtime/log")
                if event.event_type.contains('/') {
                    // Split off the function name, keep interface
                    let parts: Vec<&str> = event.event_type.rsplitn(2, '/').collect();
                    if parts.len() == 2 {
                        Some(parts[1].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        Self {
            events: Arc::new(events),
            position: Arc::new(Mutex::new(0)),
            interfaces: Arc::new(interfaces),
        }
    }

    /// Get the current event.
    pub fn current_event(&self) -> Option<ChainEvent> {
        let pos = *self.position.lock().unwrap();
        self.events.get(pos).cloned()
    }

    /// Get the output bytes for the current event.
    /// Assumes the event data contains a serialized HostFunctionCall.
    pub fn current_output(&self) -> Option<Vec<u8>> {
        let event = self.current_event()?;
        if let Ok(call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
            Some(call.output)
        } else {
            Some(event.data.clone())
        }
    }

    /// Advance to the next event.
    pub fn advance(&self) {
        let mut pos = self.position.lock().unwrap();
        *pos += 1;
    }

    /// Find the next event matching the given event type and return it.
    /// This skips any events that don't match the expected type.
    pub fn find_next_event(&self, expected_type: &str) -> Option<ChainEvent> {
        let mut pos = self.position.lock().unwrap();

        // Search from current position for an event matching the expected type
        while *pos < self.events.len() {
            let event = &self.events[*pos];
            if event.event_type == expected_type {
                let result = event.clone();
                *pos += 1; // Advance past this event
                return Some(result);
            }
            *pos += 1;
        }
        None
    }

    /// Get current position in the chain.
    pub fn current_position(&self) -> usize {
        *self.position.lock().unwrap()
    }

    /// Get total number of events.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Check if replay is complete (all events consumed).
    pub fn is_complete(&self) -> bool {
        self.current_position() >= self.events.len()
    }

    /// Verify that an actual hash matches the expected hash at current position.
    pub fn verify_hash(&self, actual_hash: &[u8]) -> Result<(), String> {
        let pos = self.current_position();
        let expected = self
            .events
            .get(pos)
            .ok_or_else(|| format!("No expected event at position {}", pos))?;

        if actual_hash != expected.hash {
            return Err(format!(
                "Hash mismatch at position {}: expected {}, got {}",
                pos,
                hex::encode(&expected.hash),
                hex::encode(actual_hash)
            ));
        }

        Ok(())
    }

    /// Get the list of interfaces discovered from the chain.
    pub fn interfaces(&self) -> Vec<String> {
        (*self.interfaces).clone()
    }
}

/// Handler that replays actors from recorded event chains.
///
/// The ReplayHandler satisfies all component imports by registering stub functions
/// that return the recorded outputs from a previous run. This enables:
///
/// - **Verification**: Confirm that a component produces the same chain given the same inputs
/// - **Debugging**: Step through a recorded execution
/// - **Testing**: Run actors without real external dependencies
#[derive(Clone)]
pub struct ReplayHandler {
    /// Replay state shared across all stub functions
    state: ReplayState,
}

impl ReplayHandler {
    /// Create a new ReplayHandler from a chain of events.
    ///
    /// The chain should be from a previous actor run, typically loaded from
    /// a saved chain file or retrieved from an actor's event history.
    pub fn new(expected_chain: Vec<ChainEvent>) -> Self {
        Self {
            state: ReplayState::new(expected_chain),
        }
    }

    /// Get the replay state for inspection.
    pub fn state(&self) -> &ReplayState {
        &self.state
    }

    /// Get progress as (current_position, total_events).
    pub fn progress(&self) -> (usize, usize) {
        (self.state.current_position(), self.state.total_events())
    }
}

/// Convert a wasmtime Val to a JSON value for serialization
fn val_to_json(val: &Val) -> serde_json::Value {
    match val {
        Val::Bool(b) => serde_json::Value::Bool(*b),
        Val::S8(n) => serde_json::Value::Number((*n as i64).into()),
        Val::U8(n) => serde_json::Value::Number((*n as u64).into()),
        Val::S16(n) => serde_json::Value::Number((*n as i64).into()),
        Val::U16(n) => serde_json::Value::Number((*n as u64).into()),
        Val::S32(n) => serde_json::Value::Number((*n as i64).into()),
        Val::U32(n) => serde_json::Value::Number((*n as u64).into()),
        Val::S64(n) => serde_json::Value::Number((*n).into()),
        Val::U64(n) => serde_json::Value::Number((*n).into()),
        Val::Float32(f) => serde_json::Number::from_f64(*f as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Val::Float64(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Val::Char(c) => serde_json::Value::String(c.to_string()),
        Val::String(s) => serde_json::Value::String(s.clone()),
        Val::List(items) => serde_json::Value::Array(items.iter().map(val_to_json).collect()),
        Val::Record(fields) => {
            let map: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|(k, v)| (k.clone(), val_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        Val::Tuple(items) => serde_json::Value::Array(items.iter().map(val_to_json).collect()),
        Val::Variant(name, value) => {
            let mut map = serde_json::Map::new();
            map.insert(
                name.clone(),
                value.as_ref().map(|v| val_to_json(v)).unwrap_or(serde_json::Value::Null),
            );
            serde_json::Value::Object(map)
        }
        Val::Enum(name) => serde_json::Value::String(name.clone()),
        Val::Option(opt) => opt
            .as_ref()
            .map(|v| val_to_json(v))
            .unwrap_or(serde_json::Value::Null),
        Val::Result(res) => match res {
            Ok(v) => {
                let mut map = serde_json::Map::new();
                map.insert(
                    "ok".to_string(),
                    v.as_ref().map(|v| val_to_json(v)).unwrap_or(serde_json::Value::Null),
                );
                serde_json::Value::Object(map)
            }
            Err(v) => {
                let mut map = serde_json::Map::new();
                map.insert(
                    "err".to_string(),
                    v.as_ref().map(|v| val_to_json(v)).unwrap_or(serde_json::Value::Null),
                );
                serde_json::Value::Object(map)
            }
        },
        Val::Flags(flags) => {
            serde_json::Value::Array(flags.iter().map(|f| serde_json::Value::String(f.clone())).collect())
        }
        Val::Resource(_) => serde_json::Value::String("<resource>".to_string()),
    }
}

/// Serialize params to JSON bytes
fn serialize_params(params: &[Val]) -> Vec<u8> {
    let json_params: Vec<serde_json::Value> = params.iter().map(val_to_json).collect();
    // For single param, don't wrap in array
    let json = if json_params.len() == 1 {
        json_params.into_iter().next().unwrap()
    } else {
        serde_json::Value::Array(json_params)
    };
    serde_json::to_vec(&json).unwrap_or_default()
}

impl Handler for ReplayHandler {
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting replay handler");

        let state = self.state.clone();

        Box::pin(async move {
            // Wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;

            let (current, total) = (state.current_position(), state.total_events());
            info!(
                "Replay handler shut down. Progress: {}/{} events",
                current, total
            );

            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up replay host functions");

        let component_type = actor_component.component.component_type();
        let engine = &actor_component.engine;

        // Collect all imports first to avoid lifetime issues with the async closures
        let imports: Vec<_> = component_type.imports(engine).collect();

        // Iterate over all imports and register stub functions
        for (import_name, import_item) in imports {
            // Clone import_name since we need it to outlive this iteration
            let import_name = import_name.to_string();

            // Skip if already satisfied by another handler
            if ctx.is_satisfied(&import_name) {
                debug!("Skipping {} - already satisfied", import_name);
                continue;
            }

            if let ComponentItem::ComponentInstance(instance_type) = import_item {
                debug!("Registering replay stubs for interface: {}", import_name);

                // Get or create the interface in the linker
                let mut interface = match actor_component.linker.instance(&import_name) {
                    Ok(i) => i,
                    Err(e) => {
                        warn!(
                            "Could not create linker instance for {}: {}",
                            import_name, e
                        );
                        continue;
                    }
                };

                // Collect exports to avoid lifetime issues
                let exports: Vec<_> = instance_type.exports(engine).collect();

                // Register a stub for each function in the interface
                for (func_name, export_item) in exports {
                    let func_name = func_name.to_string();

                    if let ComponentItem::ComponentFunc(_func_type) = export_item {
                        let full_name = format!("{}::{}", import_name, func_name);
                        let state = self.state.clone();
                        let full_name_clone = full_name.clone();

                        debug!("  Registering stub for {}", func_name);

                        // The expected event type for this function
                        // Format: "interface/function" e.g., "theater:simple/runtime/log"
                        let expected_event_type = format!("{}/{}", import_name, func_name);
                        let expected_event_type_clone = expected_event_type.clone();

                        // Capture interface and function names for the closure
                        let interface_name = import_name.clone();
                        let func_name_owned = func_name.clone();

                        // Register an async stub function
                        interface.func_new_async(
                            &func_name,
                            move |mut ctx: StoreContextMut<'_, ActorStore>,
                                  params: &[Val],
                                  _results| {
                                let state = state.clone();
                                let full_name = full_name_clone.clone();
                                let expected_type = expected_event_type_clone.clone();
                                let interface = interface_name.clone();
                                let function = func_name_owned.clone();

                                // Serialize the ACTUAL params from the actor
                                let actual_input = serialize_params(params);

                                Box::new(async move {
                                    debug!("[REPLAY] {} called, looking for event type: {}", full_name, expected_type);

                                    // Find the next event matching this function's event type
                                    // (to get the recorded output to return)
                                    let recorded_output = if let Some(event) = state.find_next_event(&expected_type) {
                                        debug!(
                                            "  Found matching event at pos {}: {}",
                                            state.current_position().saturating_sub(1),
                                            event.event_type
                                        );

                                        // Extract the recorded output from the event
                                        if let Ok(host_call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
                                            host_call.output
                                        } else {
                                            vec![]
                                        }
                                    } else {
                                        warn!(
                                            "[REPLAY] No matching event found for {} (type: {})",
                                            full_name, expected_type
                                        );
                                        vec![]
                                    };

                                    // Record a NEW event with the ACTUAL input from the actor
                                    // This allows us to verify the actor is making the same calls
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: expected_type,
                                        data: ChainEventPayload::HostFunction(HostFunctionCall::new(
                                            interface,
                                            function,
                                            actual_input,
                                            recorded_output,
                                        )),
                                    });

                                    // For functions that return (), the results slice is empty
                                    // For functions with results, we need to deserialize from chain
                                    // For now, leave results empty (works for void functions)
                                    // TODO: Proper deserialization for non-void functions

                                    Ok(())
                                })
                            },
                        )?;
                    }
                }

                // Mark this interface as satisfied
                ctx.mark_satisfied(&import_name);
            }
        }

        info!(
            "Replay handler setup complete. Tracking {} events",
            self.state.total_events()
        );

        Ok(())
    }

    fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> anyhow::Result<()> {
        // Replay handler doesn't add export functions
        Ok(())
    }

    fn name(&self) -> &str {
        "replay"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // Return None to indicate "match all imports"
        // The ReplayHandler will register stubs for any unsatisfied imports
        // during setup_host_functions
        None
    }

    fn exports(&self) -> Option<Vec<String>> {
        // Replay handler doesn't expect any exports
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_state_creation() {
        let events = vec![ChainEvent {
            hash: vec![1, 2, 3, 4],
            parent_hash: None,
            event_type: "theater:simple/runtime/log".to_string(),
            data: vec![],
        }];

        let state = ReplayState::new(events);
        assert_eq!(state.current_position(), 0);
        assert_eq!(state.total_events(), 1);
        assert!(!state.is_complete());

        let interfaces = state.interfaces();
        assert!(interfaces.contains(&"theater:simple/runtime".to_string()));
    }

    #[test]
    fn test_replay_state_advance() {
        let events = vec![
            ChainEvent {
                hash: vec![1, 2, 3, 4],
                parent_hash: None,
                event_type: "test".to_string(),
                data: vec![],
            },
            ChainEvent {
                hash: vec![5, 6, 7, 8],
                parent_hash: Some(vec![1, 2, 3, 4]),
                event_type: "test2".to_string(),
                data: vec![],
            },
        ];

        let state = ReplayState::new(events);
        assert_eq!(state.current_position(), 0);

        state.advance();
        assert_eq!(state.current_position(), 1);

        state.advance();
        assert_eq!(state.current_position(), 2);
        assert!(state.is_complete());
    }

    #[test]
    fn test_replay_handler_creation() {
        let events = vec![ChainEvent {
            hash: vec![1, 2, 3, 4],
            parent_hash: None,
            event_type: "theater:simple/runtime/log".to_string(),
            data: vec![],
        }];

        let handler = ReplayHandler::new(events);
        assert_eq!(handler.state().total_events(), 1);

        let (current, total) = handler.progress();
        assert_eq!(current, 0);
        assert_eq!(total, 1);
    }
}
