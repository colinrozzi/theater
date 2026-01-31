//! # Replay Handler
//!
//! The ReplayHandler is a special handler that replays actors from recorded event chains.
//!
//! ## Status: Needs Reimplementation for Composite Runtime
//!
//! This handler was originally implemented for the wasmtime Component Model runtime.
//! It needs to be reimplemented for the Composite (Graph ABI) runtime.
//!
//! The wasmtime-specific implementation is preserved in git history for reference.
//!
//! ## Usage (once reimplemented)
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

use tracing::warn;

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::chain::ChainEvent;
use crate::pack_bridge::{HostLinkerBuilder, LinkerError, PackInstance};
use crate::handler::{Handler, HandlerContext, SharedActorInstance};
use crate::shutdown::ShutdownReceiver;

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

    /// Get the output for the current event.
    /// Assumes the event data contains a serialized HostFunctionCall.
    pub fn current_output(&self) -> Option<pack::abi::Value> {
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
/// ## Status: Needs Reimplementation
///
/// This handler needs to be reimplemented for the Composite (Graph ABI) runtime.
/// The wasmtime-specific implementation is preserved in git history.
///
/// When reimplemented, the ReplayHandler will:
/// - Satisfy all component imports by registering stub functions
/// - Return recorded outputs from a previous run
/// - Enable verification, debugging, and testing without real external dependencies
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

impl Handler for ReplayHandler {
    fn create_instance(
        &self,
        _config: Option<&crate::config::actor_manifest::HandlerConfig>,
    ) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        warn!("ReplayHandler::start() - Replay not yet implemented for Composite runtime");

        Box::pin(async move {
            // Just wait for shutdown since replay isn't implemented yet
            shutdown_receiver.wait_for_shutdown().await;
            Ok(())
        })
    }

    fn setup_host_functions_composite(
        &mut self,
        _builder: &mut HostLinkerBuilder<'_, ActorStore>,
        _ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        // TODO: Implement replay for Composite runtime
        // This should register stub functions that return recorded outputs
        warn!("ReplayHandler::setup_host_functions_composite() - Not yet implemented for Composite runtime");
        Ok(())
    }

    fn register_exports_composite(&self, _instance: &mut PackInstance) -> anyhow::Result<()> {
        // Replay handler doesn't add export functions
        Ok(())
    }

    fn name(&self) -> &str {
        "replay"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // Return None to indicate "match all imports"
        // The ReplayHandler will register stubs for any unsatisfied imports
        None
    }

    fn exports(&self) -> Option<Vec<String>> {
        // Replay handler doesn't expect any exports
        None
    }

    fn supports_composite(&self) -> bool {
        // Mark as supporting composite even though implementation is pending
        // This allows the handler to be registered and will log warnings when used
        true
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
