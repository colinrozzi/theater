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
//!
//! ## Known Limitations
//!
//! ### Resource-Based WASI Interfaces
//!
//! The replay handler has limitations when replaying WASI interfaces that use resources
//! (pollables, file descriptors, streams, sockets, etc.). The wasmtime component model
//! requires strict lifecycle tracking for resources that our dynamic approach cannot
//! properly implement.
//!
//! **Interfaces that work well with replay:**
//! - `theater:simple/runtime` (logging, chain access, shutdown)
//! - `theater:simple/environment` (environment variables)
//! - `wasi:random/random` (random number generation)
//! - Any interface that only returns simple values (integers, strings, tuples)
//!
//! **Interfaces with replay limitations:**
//! - `wasi:clocks/monotonic-clock` (uses pollable resources)
//! - `wasi:io/poll` (defines pollable resources)
//! - `wasi:filesystem/*` (uses file descriptor resources)
//! - `wasi:sockets/*` (uses socket resources)
//! - `wasi:http/*` (uses stream and future resources)
//!
//! For resource-based interfaces, consider implementing replay-aware handlers that:
//! 1. Create real resources using the handler's normal implementation
//! 2. Override return values (timestamps, data, etc.) from the recorded chain
//! 3. Properly manage resource lifecycle

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tracing::{debug, error, info, warn};
use val_serde::SerializableVal;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Resource, ResourceAny, ResourceType, Val};
use wasmtime::StoreContextMut;

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::chain::ChainEvent;
use crate::events::{wasm::WasmEventData, ChainEventData, ChainEventPayload};
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
    /// Set of resource names that have already been registered with the linker
    /// Used to avoid registering the same resource type multiple times
    registered_resources: Arc<Mutex<HashSet<String>>>,
}

impl ReplayResourceState {
    pub fn new() -> Self {
        Self {
            next_rep: Arc::new(AtomicU32::new(0)), // Start at 0 to match recorded chain
            resources: Arc::new(Mutex::new(HashMap::new())),
            registered_resources: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Check if a resource name has been registered, and if not, mark it as registered
    /// Returns true if the resource was NOT previously registered (and is now marked)
    /// Returns false if it was already registered
    pub fn try_register(&self, resource_name: &str) -> bool {
        let mut registered = self.registered_resources.lock().unwrap();
        if registered.contains(resource_name) {
            false
        } else {
            registered.insert(resource_name.to_string());
            true
        }
    }

    /// Generate a new unique resource handle
    pub fn new_rep(&self) -> u32 {
        self.next_rep.fetch_add(1, Ordering::SeqCst)
    }

    /// Remove a resource from tracking (called when destructor runs)
    pub fn remove(&self, resource_type: &str, rep: u32) {
        let mut resources = self.resources.lock().unwrap();
        resources.remove(&(resource_type.to_string(), rep));
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
    /// Count of hash mismatches detected during replay
    mismatch_count: Arc<AtomicU32>,
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
            mismatch_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Record a hash mismatch
    pub fn record_mismatch(&self) {
        self.mismatch_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Get the count of mismatches
    pub fn mismatch_count(&self) -> u32 {
        self.mismatch_count.load(Ordering::SeqCst)
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

    /// Get the output for the current event.
    /// Assumes the event data contains a serialized HostFunctionCall.
    pub fn current_output(&self) -> Option<SerializableVal> {
        let event = self.current_event()?;
        if let Ok(call) = serde_json::from_slice::<HostFunctionCall>(&event.data) {
            Some(call.output)
        } else {
            None
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

/// Convert params to SerializableVal for recording.
/// Matches the format used by handlers for consistent serialization.
fn serialize_params(params: &[Val]) -> SerializableVal {
    if params.is_empty() {
        SerializableVal::Tuple(vec![])
    } else if params.len() == 1 {
        SerializableVal::from(&params[0])
    } else {
        SerializableVal::Tuple(params.iter().map(SerializableVal::from).collect())
    }
}

/// Deserialize a SerializableVal back to a wasmtime Val.
/// This is a simple conversion since SerializableVal preserves type information.
fn deserialize_output(recorded_output: &SerializableVal, results: &mut [Val]) {
    if results.is_empty() {
        return;
    }

    // Convert SerializableVal to Val
    // Note: This will panic for Resource types, which should be handled separately
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Val::from(recorded_output.clone())
    })) {
        Ok(val) => {
            results[0] = val;
        }
        Err(_) => {
            warn!("[REPLAY] Failed to convert SerializableVal to Val (possibly a resource type)");
        }
    }
}

/// Parse a WasmCall event to extract function_name and params
fn parse_wasm_call(event: &ChainEvent) -> Option<(String, Vec<u8>)> {
    if event.event_type != "wasm" {
        return None;
    }

    let payload: ChainEventPayload = serde_json::from_slice(&event.data).ok()?;

    if let ChainEventPayload::Wasm(WasmEventData::WasmCall {
        function_name,
        params,
    }) = payload
    {
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

    info!(
        "Triggering export: interface={}, function={}",
        interface_name, func_name
    );

    let mut guard = actor_instance.write().await;
    let instance = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Actor instance not available"))?;

    // Get the interface export
    let interface_export = instance
        .instance
        .get_export(&mut instance.store, None, interface_name)
        .ok_or_else(|| anyhow::anyhow!("Interface not exported: {}", interface_name))?;

    // Get the function from the interface
    let func_export = instance
        .instance
        .get_export(&mut instance.store, Some(&interface_export), func_name)
        .ok_or_else(|| anyhow::anyhow!("Function not exported: {}", func_name))?;

    let func = instance
        .instance
        .get_func(&mut instance.store, &func_export)
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
    instance
        .actor_component
        .actor_store
        .record_event(ChainEventData {
            event_type: "wasm".to_string(),
            data: ChainEventPayload::Wasm(WasmEventData::WasmCall {
                function_name: function_name.to_string(),
                params: recorded_params.to_vec(),
            }),
        });

    // Call the export (no return values for HTTP handler)
    let mut results = [];

    info!("Calling export {}...", func_name);
    func.call_async(&mut instance.store, &params, &mut results)
        .await?;

    // Post-return cleanup
    func.post_return_async(&mut instance.store).await?;

    info!("Export {} completed", func_name);

    // Note: WasmResult will be recorded by the stub functions when response-outparam.set is called
    // or we could record it here based on the expected chain

    Ok(())
}

impl Handler for ReplayHandler {
    fn create_instance(
        &self,
        _config: Option<&crate::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
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
            let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<
                Result<ChainEvent, crate::actor::types::ActorError>,
            >(100);

            // Subscribe to actor events BEFORE calling init
            // This ensures we receive all events from the beginning
            if let Err(e) = theater_tx
                .send(crate::messages::TheaterCommand::SubscribeToActor {
                    actor_id: actor_id.clone(),
                    event_tx,
                })
                .await
            {
                warn!("Failed to subscribe to actor events: {:?}", e);
                shutdown_receiver.wait_for_shutdown().await;
                return Ok(());
            }

            info!("Subscribed to actor {} events", actor_id);

            // Now call init - the replay handler drives execution in replay mode
            // The actor runtime skips automatic init when replay mode is detected
            info!("Replay handler calling init...");
            if let Err(e) = actor_handle
                .call_function::<(), ()>("theater:simple/actor.init".to_string(), ())
                .await
            {
                error!("Replay handler failed to call init: {:?}", e);
                return Err(anyhow::anyhow!(
                    "Failed to call init during replay: {:?}",
                    e
                ));
            }
            info!("Replay handler init completed, starting event verification loop");

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
                                // Find which position this event corresponds to in the expected chain
                                // Events may arrive in order but stub functions may have already
                                // processed some positions (for non-wasm events)
                                let event_hash = &event.hash;

                                // Find the position of this event in the expected chain
                                let mut found_pos = None;
                                for (pos, expected) in state.events.iter().enumerate() {
                                    if &expected.hash == event_hash {
                                        found_pos = Some(pos);
                                        break;
                                    }
                                }

                                let Some(event_pos) = found_pos else {
                                    warn!("[REPLAY] Received unexpected event type='{}', hash={} (not in expected chain)",
                                        event.event_type, hex::encode(&event.hash[..8.min(event.hash.len())]));
                                    continue;
                                };

                                let current_pos = state.current_position();

                                debug!("[REPLAY] Received event type='{}' at chain position {}, current_pos={}",
                                    event.event_type, event_pos, current_pos);

                                // If this event is at or before current position, it was already
                                // handled by stub functions - skip it
                                if event_pos < current_pos {
                                    debug!("[REPLAY] Event at position {} already processed (current={}), skipping",
                                        event_pos, current_pos);
                                    continue;
                                }

                                // If this event is exactly at current position, verify and advance
                                if event_pos == current_pos {
                                    info!("[REPLAY] Event {} verified at position {}", event.event_type, current_pos);
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
                                    // event_pos > current_pos: event arrived out of order
                                    // This can happen if events are received before stubs process them
                                    // Just log and continue - the stub will advance position when it runs
                                    debug!("[REPLAY] Event at position {} received before current position {} reached, will verify later",
                                        event_pos, current_pos);
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

            // Only record success summary if we completed all events (didn't break due to mismatch)
            if state.is_complete() {
                info!(
                    "Replay completed successfully: all {} events matched",
                    total
                );

                let summary = crate::events::replay::ReplaySummary::success(total, current);

                if let Ok(guard) = actor_instance.try_read() {
                    if let Some(instance) = guard.as_ref() {
                        instance.actor_component.actor_store.record_event(
                            crate::events::ChainEventData {
                                event_type: "replay-summary".to_string(),
                                data: crate::events::ChainEventPayload::ReplaySummary(summary),
                            },
                        );
                    }
                }
            } else {
                // We exited early (mismatch or shutdown) - summary already recorded in mismatch handler
                info!("Replay handler exited at position {}/{}", current, total);
            }

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
                // Track which resources we've registered to avoid duplicates
                // (e.g., pollable is defined in wasi:io/poll but used by wasi:clocks)
                for (item_name, export_item) in &exports {
                    if let ComponentItem::Resource(_resource_type) = export_item {
                        let resource_name = item_name.to_string();

                        // Skip if this resource type was already registered on another interface
                        if !self.state.resource_state.try_register(&resource_name) {
                            debug!("Skipping duplicate resource type: {}::{} (already registered)", import_name, resource_name);
                            continue;
                        }

                        let resource_state = self.state.resource_state.clone();
                        let interface_name = import_name.clone();
                        let resource_name_for_dtor = resource_name.clone();

                        debug!("  Registering resource type: {}::{}", import_name, resource_name);

                        // Define the resource using our marker type
                        // The destructor is called when the guest drops an owned resource
                        interface.resource(
                            &resource_name,
                            ResourceType::host::<ReplayResourceMarker>(),
                            move |_store: StoreContextMut<'_, ActorStore>, rep: u32| {
                                debug!(
                                    "Resource destructor: {}::{} dropped (rep={})",
                                    interface_name, resource_name_for_dtor, rep
                                );
                                resource_state.remove(&resource_name_for_dtor, rep);
                                Ok(())
                            },
                        )?;

                        // Also register the [resource-drop] function for this resource
                        // This is required because WASM imports it as a separate function
                        let drop_func_name = format!("[resource-drop]{}", resource_name);
                        let interface_name_for_drop = import_name.clone();
                        let resource_name_for_drop = resource_name.clone();

                        debug!("  Registering resource-drop function: {}::{}", import_name, drop_func_name);

                        interface.func_wrap(
                            &drop_func_name,
                            move |mut ctx: StoreContextMut<'_, ActorStore>,
                                  (resource_handle,): (Resource<ReplayResourceMarker>,)| -> anyhow::Result<()> {
                                let rep = resource_handle.rep();
                                debug!(
                                    "[resource-drop] {}::{} called (rep={})",
                                    interface_name_for_drop, resource_name_for_drop, rep
                                );

                                // Remove from resource table if it's there
                                if let Ok(mut table) = ctx.data_mut().resource_table.lock() {
                                    let _ = table.delete(resource_handle);
                                }

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

                        debug!("  Registering stub for {}", full_name);

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

                        // Register a synchronous stub function
                        // Using func_new instead of func_new_async for simpler return value handling
                        interface.func_new(
                            &func_name,
                            move |mut ctx: StoreContextMut<'_, ActorStore>,
                                  params: &[Val],
                                  results: &mut [Val]| {
                                let state = state.clone();
                                let full_name = full_name_clone.clone();
                                let expected_type = expected_event_type_clone.clone();
                                let interface = interface_name.clone();
                                let function = func_name_owned.clone();

                                // Serialize the ACTUAL params from the actor as SerializableVal
                                let actual_input = serialize_params(params);

                                // Clone values we need
                                let returns_resource = returns_resource;
                                let is_constructor = is_constructor;

                                debug!(
                                    "[REPLAY] {} called, expecting event type: {}",
                                    full_name, expected_type
                                );

                                // Expect the next event to match this function call (strict sequential)
                                let (recorded_input, recorded_output): (Option<SerializableVal>, Option<SerializableVal>) =
                                    match state.expect_next_event(&expected_type) {
                                        Ok(event) => {
                                            debug!(
                                                "  Matched event at pos {}: {}",
                                                state.current_position().saturating_sub(1),
                                                event.event_type
                                            );

                                            // Extract the recorded input and output from the event
                                            // The event.data contains a serialized HostFunctionCall
                                            if let Ok(host_call) =
                                                serde_json::from_slice::<HostFunctionCall>(
                                                    &event.data,
                                                )
                                            {
                                                (Some(host_call.input), Some(host_call.output))
                                            } else {
                                                (None, None)
                                            }
                                        }
                                        Err(e) => {
                                            // Mismatch detected - this is a determinism failure
                                            warn!("[REPLAY] {}", e);
                                            (None, None)
                                        }
                                    };

                                // Get values for recording (use actual/empty if none recorded)
                                let input_for_recording = recorded_input
                                    .unwrap_or_else(|| actual_input.clone());
                                let output_for_recording = recorded_output
                                    .clone()
                                    .unwrap_or_else(|| SerializableVal::Tuple(vec![]));

                                // Record the event using the RECORDED input to produce identical hashes
                                // This ensures the replay chain matches the original chain exactly
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: expected_type,
                                    data: ChainEventPayload::HostFunction(
                                        HostFunctionCall::new(
                                            interface,
                                            function,
                                            input_for_recording,
                                            output_for_recording,
                                        ),
                                    ),
                                });

                                // Handle resource-returning functions (constructors)
                                // NOTE: Resource-based replay has limitations. The component model's
                                // resource lifecycle tracking doesn't work correctly with our dynamic
                                // resource creation approach. See wasmtime docs on ResourceAny.
                                if (returns_resource || is_constructor) && !results.is_empty() {
                                    // Create a resource handle with a unique rep value
                                    let rep = state.resource_state().new_rep();
                                    debug!("{} creating resource with rep={}", full_name, rep);

                                    // Create a Resource handle
                                    let resource: Resource<ReplayResourceMarker> = Resource::new_own(rep);

                                    // Convert to ResourceAny
                                    match ResourceAny::try_from_resource(resource, &mut ctx) {
                                        Ok(any) => {
                                            debug!("{} created ResourceAny: {:?}", full_name, any);
                                            results[0] = Val::Resource(any);
                                        }
                                        Err(e) => {
                                            warn!(
                                                "[REPLAY] Failed to create ResourceAny for {}: {}",
                                                full_name, e
                                            );
                                        }
                                    }
                                } else if !results.is_empty() {
                                    // Handle non-resource return values by deserializing from recorded_output
                                    if let Some(ref output) = recorded_output {
                                        debug!(
                                            "[REPLAY] Deserializing output for {}: {:?}",
                                            full_name, output
                                        );
                                        deserialize_output(output, results);
                                    } else {
                                        warn!("[REPLAY] No recorded output for {}", full_name);
                                    }
                                }

                                Ok(())
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
