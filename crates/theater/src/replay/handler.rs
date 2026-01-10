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

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Resource, ResourceAny, ResourceType, Val};
use wasmtime::StoreContextMut;

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::chain::ChainEvent;
use crate::events::{ChainEventData, ChainEventPayload, wasm::WasmEventData};
use crate::handler::{Handler, HandlerContext, SharedActorInstance};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};

use super::HostFunctionCall;

/// Marker type for replay resources.
/// This is used as a type parameter for Resource<T> when creating resources during replay.
/// The actual backing data is tracked separately in ReplayResourceState.
pub struct ReplayResourceMarker;

/// State for tracking replay resources across the replay session.
#[derive(Clone, Default)]
pub struct ReplayResourceState {
    /// Counter for generating unique resource handles (rep values)
    next_rep: Arc<AtomicU32>,
    /// Map of (resource_type_name, rep) -> optional recorded state
    /// For most resources during replay, we don't need actual state - just the handle
    #[allow(dead_code)]
    resources: Arc<Mutex<HashMap<(String, u32), Vec<u8>>>>,
}

impl ReplayResourceState {
    pub fn new() -> Self {
        Self {
            next_rep: Arc::new(AtomicU32::new(1)), // Start at 1, 0 often means null
            resources: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Generate a new unique resource handle
    pub fn new_rep(&self) -> u32 {
        self.next_rep.fetch_add(1, Ordering::SeqCst)
    }

    /// Track a new resource
    #[allow(dead_code)]
    pub fn track(&self, resource_type: &str, rep: u32, data: Vec<u8>) {
        let mut resources = self.resources.lock().unwrap();
        resources.insert((resource_type.to_string(), rep), data);
    }

    /// Remove a resource (on drop)
    #[allow(dead_code)]
    pub fn remove(&self, resource_type: &str, rep: u32) -> Option<Vec<u8>> {
        let mut resources = self.resources.lock().unwrap();
        resources.remove(&(resource_type.to_string(), rep))
    }
}

/// Shared state for tracking replay position across all stub functions.
#[derive(Clone)]
pub struct ReplayState {
    /// The expected chain events
    events: Arc<Vec<ChainEvent>>,
    /// Current position in the chain
    position: Arc<Mutex<usize>>,
    /// List of interfaces discovered from the chain
    interfaces: Arc<Vec<String>>,
    /// Resource state for tracking handles during replay
    resource_state: ReplayResourceState,
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
            resource_state: ReplayResourceState::new(),
        }
    }

    /// Get the resource state for creating/tracking resources during replay
    pub fn resource_state(&self) -> &ReplayResourceState {
        &self.resource_state
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

    /// Expect the next event to match the given event type.
    /// Returns the event if it matches, or an error if it doesn't.
    /// This enforces strict sequential ordering - no skipping allowed.
    pub fn expect_next_event(&self, expected_type: &str) -> Result<ChainEvent, String> {
        let mut pos = self.position.lock().unwrap();

        if *pos >= self.events.len() {
            return Err(format!(
                "Replay chain exhausted. Expected event type: {}, but no more events at position {}",
                expected_type, *pos
            ));
        }

        let event = &self.events[*pos];

        // Skip "wasm" events (init events handled separately)
        // and skip events we've already processed
        if event.event_type == "wasm" {
            *pos += 1;
            drop(pos);
            return self.expect_next_event(expected_type);
        }

        if event.event_type != expected_type {
            return Err(format!(
                "Replay mismatch at position {}:\n  Expected: {}\n  Got: {}\nThis indicates nondeterminism in the actor.",
                *pos, expected_type, event.event_type
            ));
        }

        let result = event.clone();
        *pos += 1;
        Ok(result)
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

/// Try to deserialize recorded output bytes back to Val
/// This handles common patterns from the HTTP host implementation
fn deserialize_recorded_output(recorded_output: &[u8], results: &mut [Val]) {
    if results.is_empty() || recorded_output.is_empty() {
        return;
    }

    // Try to parse as JSON string first
    let output_str = match String::from_utf8(recorded_output.to_vec()) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Try parsing as JSON
    let json_value: serde_json::Value = match serde_json::from_str(&output_str) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Handle different patterns based on the JSON structure
    match &json_value {
        // Enum/variant pattern: "Type::Variant" -> Val::Enum("variant")
        serde_json::Value::String(s) if s.contains("::") => {
            let parts: Vec<&str> = s.split("::").collect();
            if parts.len() >= 2 {
                // Extract the variant name (lowercase for component model)
                let variant_name = parts[parts.len() - 1].to_lowercase();
                // For simple enums with no payload
                results[0] = Val::Enum(variant_name);
                debug!("[REPLAY] Deserialized enum variant: {}", s);
            }
        }
        // Plain string -> could be Val::String or Val::Option(Some(String))
        serde_json::Value::String(s) => {
            // Assume it's an Option<String> returning Some
            results[0] = Val::Option(Some(Box::new(Val::String(s.clone()))));
            debug!("[REPLAY] Deserialized as Option<String>: {}", s);
        }
        // null -> Val::Option(None)
        serde_json::Value::Null => {
            results[0] = Val::Option(None);
            debug!("[REPLAY] Deserialized as None");
        }
        // Array -> could be list of bytes or other list
        serde_json::Value::Array(arr) => {
            // Try to convert to list of u8 (common for HTTP bodies)
            let bytes: Vec<Val> = arr.iter()
                .filter_map(|v| v.as_u64().map(|n| Val::U8(n as u8)))
                .collect();
            if bytes.len() == arr.len() {
                results[0] = Val::List(bytes);
                debug!("[REPLAY] Deserialized as list of {} bytes", arr.len());
            }
        }
        // Boolean
        serde_json::Value::Bool(b) => {
            results[0] = Val::Bool(*b);
            debug!("[REPLAY] Deserialized as bool: {}", b);
        }
        // Number
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                results[0] = Val::U32(i as u32);
                debug!("[REPLAY] Deserialized as u32: {}", i);
            }
        }
        // Result pattern: {"ok": value} or {"err": value}
        serde_json::Value::Object(map) => {
            if let Some(ok_val) = map.get("ok") {
                // Result::Ok
                let inner = match ok_val {
                    serde_json::Value::Null => None,
                    _ => Some(Box::new(Val::String(ok_val.to_string()))),
                };
                results[0] = Val::Result(Ok(inner));
                debug!("[REPLAY] Deserialized as Result::Ok");
            } else if let Some(err_val) = map.get("err") {
                // Result::Err
                let inner = match err_val {
                    serde_json::Value::Null => None,
                    _ => Some(Box::new(Val::String(err_val.to_string()))),
                };
                results[0] = Val::Result(Err(inner));
                debug!("[REPLAY] Deserialized as Result::Err");
            }
        }
    }
}

/// Parse a WasmCall event to extract function_name and params
fn parse_wasm_call(event: &ChainEvent) -> Option<(String, Vec<u8>)> {
    if event.event_type != "wasm" {
        return None;
    }

    let payload: ChainEventPayload = serde_json::from_slice(&event.data).ok()?;

    if let ChainEventPayload::Wasm(WasmEventData::WasmCall { function_name, params }) = payload {
        Some((function_name, params))
    } else {
        None
    }
}

/// Trigger an export function on the component
async fn trigger_export(
    actor_instance: &SharedActorInstance,
    function_name: &str,
    recorded_params: &[u8],
) -> anyhow::Result<()> {
    // Parse interface and function from function_name
    // Format: "wasi:http/incoming-handler@0.2.0/handle" -> interface="wasi:http/incoming-handler@0.2.0", func="handle"
    let parts: Vec<&str> = function_name.rsplitn(2, '/').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid export function name format: {}", function_name);
    }
    let func_name = parts[0];
    let interface_name = parts[1];

    info!("Triggering export: interface={}, function={}", interface_name, func_name);

    let mut guard = actor_instance.write().await;
    let instance = guard.as_mut()
        .ok_or_else(|| anyhow::anyhow!("Actor instance not available"))?;

    // Get the interface export
    let interface_export = instance.instance.get_export(
        &mut instance.store,
        None,
        interface_name,
    ).ok_or_else(|| anyhow::anyhow!("Interface not exported: {}", interface_name))?;

    // Get the function from the interface
    let func_export = instance.instance.get_export(
        &mut instance.store,
        Some(&interface_export),
        func_name,
    ).ok_or_else(|| anyhow::anyhow!("Function not exported: {}", func_name))?;

    let func = instance.instance.get_func(&mut instance.store, &func_export)
        .ok_or_else(|| anyhow::anyhow!("Failed to get Func for {}", func_name))?;

    // For exports that take resources (like HTTP handler), we need to create stub resources
    // HTTP handler takes 2 resource parameters: incoming-request and response-outparam
    // The stub functions will return recorded data when the component calls methods on them
    let mut params: Vec<Val> = Vec::new();

    // Create stub resources for each expected parameter
    // For HTTP: param 0 = incoming-request (borrow), param 1 = response-outparam (own)
    // The rep values are used to identify resources - we use simple incrementing values
    for i in 0..2 {
        let rep = i as u32;
        let resource = Resource::<ReplayResourceMarker>::new_own(rep);
        let resource_any = ResourceAny::try_from_resource(resource, &mut instance.store)?;
        params.push(Val::Resource(resource_any));
        debug!("Created stub resource for param {} (rep={})", i, rep);
    }

    // Record WasmCall BEFORE calling the export (matching server.rs behavior)
    // This ensures the replay chain matches the original chain structure
    instance.actor_component.actor_store.record_event(ChainEventData {
        event_type: "wasm".to_string(),
        data: ChainEventPayload::Wasm(WasmEventData::WasmCall {
            function_name: function_name.to_string(),
            params: recorded_params.to_vec(),
        }),
    });

    // Call the export (no return values for HTTP handler)
    let mut results = [];

    info!("Calling export {}...", func_name);
    func.call_async(&mut instance.store, &params, &mut results).await?;

    // Post-return cleanup
    func.post_return_async(&mut instance.store).await?;

    info!("Export {} completed", func_name);

    // Note: WasmResult will be recorded by the stub functions when response-outparam.set is called
    // or we could record it here based on the expected chain

    Ok(())
}

impl Handler for ReplayHandler {
    fn create_instance(&self, _config: Option<&crate::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting replay handler");

        let state = self.state.clone();

        Box::pin(async move {
            // Get actor ID and theater_tx to subscribe to events
            let (actor_id, theater_tx) = {
                let guard = actor_instance.read().await;
                match &*guard {
                    Some(instance) => {
                        let id = instance.actor_component.actor_store.id.clone();
                        let tx = instance.actor_component.actor_store.theater_tx.clone();
                        (id, tx)
                    }
                    None => {
                        warn!("Actor instance not available for replay subscription");
                        shutdown_receiver.wait_for_shutdown().await;
                        return Ok(());
                    }
                }
            };

            // Create channel to receive events
            let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Result<ChainEvent, crate::actor::types::ActorError>>(100);

            // Subscribe to actor events
            if let Err(e) = theater_tx.send(crate::messages::TheaterCommand::SubscribeToActor {
                actor_id: actor_id.clone(),
                event_tx,
            }).await {
                warn!("Failed to subscribe to actor events: {:?}", e);
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            }

            info!("Subscribed to actor {} events, starting replay loop", actor_id);

            // Event-driven replay loop
            let mut shutdown_rx = shutdown_receiver.receiver;
            loop {
                tokio::select! {
                    // Check for shutdown
                    _ = &mut shutdown_rx => {
                        info!("Replay handler received shutdown signal");
                        break;
                    }

                    // Check if we're done with the chain
                    _ = async {}, if state.is_complete() => {
                        info!("Replay chain complete!");
                        break;
                    }

                    // Process incoming events
                    event_result = event_rx.recv() => {
                        match event_result {
                            Some(Ok(event)) => {
                                // Compare event against expected chain
                                let current_pos = state.current_position();
                                if current_pos >= state.total_events() {
                                    warn!("Received event beyond chain length: {}", event.event_type);
                                    continue;
                                }

                                let expected = &state.events[current_pos];

                                if event.hash == expected.hash {
                                    debug!("Event {} matches expected (pos {})", event.event_type, current_pos);
                                    state.advance();

                                    // Check if next event is a WasmCall we need to trigger
                                    let next_pos = state.current_position();
                                    if next_pos < state.total_events() {
                                        let next_event = &state.events[next_pos];
                                        if let Some((function_name, params)) = parse_wasm_call(next_event) {
                                            // Skip init calls
                                            if !function_name.contains("actor.init") {
                                                info!("Next event is export WasmCall: {}, triggering...", function_name);

                                                if let Err(e) = trigger_export(
                                                    &actor_instance,
                                                    &function_name,
                                                    &params
                                                ).await {
                                                    warn!("Failed to trigger export {}: {:?}", function_name, e);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    warn!(
                                        "Event hash mismatch at position {}!\n  Expected: {:?}\n  Got: {:?}",
                                        current_pos,
                                        hex::encode(&expected.hash[..8.min(expected.hash.len())]),
                                        hex::encode(&event.hash[..8.min(event.hash.len())])
                                    );
                                    // Still advance to keep trying
                                    state.advance();
                                }
                            }
                            Some(Err(e)) => {
                                warn!("Received error event: {:?}", e);
                            }
                            None => {
                                info!("Event channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            let (current, total) = (state.current_position(), state.total_events());
            info!(
                "Replay handler finished. Progress: {}/{} events",
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
        info!("Setting up replay host functions (with WASI resource support)");

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

                // First pass: Register all resource types
                for (item_name, export_item) in &exports {
                    if let ComponentItem::Resource(_resource_type) = export_item {
                        let resource_name = item_name.to_string();
                        let resource_state = self.state.resource_state.clone();
                        let interface_name = import_name.clone();
                        let resource_name_for_dtor = resource_name.clone();

                        debug!("  Registering resource type: {}", resource_name);

                        // Define the resource using our marker type
                        // The destructor is called when the guest drops an owned resource
                        interface.resource(
                            &resource_name,
                            ResourceType::host::<ReplayResourceMarker>(),
                            move |_store: StoreContextMut<'_, ActorStore>, rep: u32| {
                                debug!(
                                    "[REPLAY] Resource {}::{} dropped (rep={})",
                                    interface_name, resource_name_for_dtor, rep
                                );
                                resource_state.remove(&resource_name_for_dtor, rep);
                                Ok(())
                            },
                        )?;
                    }
                }

                // Second pass: Register all functions (including resource methods)
                for (func_name, export_item) in exports {
                    let func_name = func_name.to_string();

                    if let ComponentItem::ComponentFunc(func_type) = export_item {
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

                        // Check if this function returns a resource (constructor pattern)
                        let returns_resource = func_type
                            .results()
                            .any(|r| matches!(r, wasmtime::component::types::Type::Own(_) | wasmtime::component::types::Type::Borrow(_)));

                        // Check if this is a constructor (starts with [constructor])
                        let is_constructor = func_name.starts_with("[constructor]");

                        // Register an async stub function
                        interface.func_new_async(
                            &func_name,
                            move |mut ctx: StoreContextMut<'_, ActorStore>,
                                  params: &[Val],
                                  results: &mut [Val]| {
                                let state = state.clone();
                                let full_name = full_name_clone.clone();
                                let expected_type = expected_event_type_clone.clone();
                                let interface = interface_name.clone();
                                let function = func_name_owned.clone();

                                // Serialize the ACTUAL params from the actor
                                let actual_input = serialize_params(params);

                                // Clone values we need in the async block
                                let returns_resource = returns_resource;
                                let is_constructor = is_constructor;

                                Box::new(async move {
                                    debug!("[REPLAY] {} called, expecting event type: {}", full_name, expected_type);

                                    // Expect the next event to match this function call (strict sequential)
                                    let recorded_output = match state.expect_next_event(&expected_type) {
                                        Ok(event) => {
                                            debug!(
                                                "  Matched event at pos {}: {}",
                                                state.current_position().saturating_sub(1),
                                                event.event_type
                                            );

                                            // Extract the recorded output from the event
                                            if let Ok(host_call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
                                                host_call.output
                                            } else {
                                                vec![]
                                            }
                                        }
                                        Err(e) => {
                                            // Mismatch detected - this is a determinism failure
                                            warn!("[REPLAY] {}", e);
                                            // For now, return empty and continue - could make this fatal
                                            vec![]
                                        }
                                    };

                                    // Record a NEW event with the ACTUAL input from the actor
                                    // This allows us to verify the actor is making the same calls
                                    ctx.data_mut().record_event(ChainEventData {
                                        event_type: expected_type,
                                        data: ChainEventPayload::HostFunction(HostFunctionCall::new(
                                            interface,
                                            function,
                                            actual_input,
                                            recorded_output.clone(),
                                        )),
                                    });

                                    // Handle resource-returning functions (constructors)
                                    if (returns_resource || is_constructor) && !results.is_empty() {
                                        // Create a new resource handle
                                        let rep = state.resource_state().new_rep();
                                        debug!("[REPLAY] Creating resource with rep={}", rep);

                                        // Create a Resource with this rep
                                        let resource: Resource<ReplayResourceMarker> = Resource::new_own(rep);

                                        // Convert to ResourceAny
                                        match ResourceAny::try_from_resource(resource, &mut ctx) {
                                            Ok(any) => {
                                                results[0] = Val::Resource(any);
                                            }
                                            Err(e) => {
                                                warn!("[REPLAY] Failed to create ResourceAny: {}", e);
                                            }
                                        }
                                    } else if !results.is_empty() {
                                        // Handle non-resource return values by deserializing from recorded_output
                                        deserialize_recorded_output(&recorded_output, results);
                                    }

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
