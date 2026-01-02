//! # Replay Module
//!
//! This module provides types and handlers for recording and replaying actor executions.
//! The key insight is that the chain + component binary contain everything needed for replay:
//!
//! - **Chain**: Records which functions were called and their I/O (serialized bytes)
//! - **Component**: Contains the type definitions (extracted at replay time)
//!
//! ## Recording
//!
//! During normal execution, handlers record host function calls using `HostFunctionCall`:
//!
//! ```ignore
//! ctx.data_mut().record_host_function_call(
//!     "theater:simple/timing",
//!     "now",
//!     &(),      // input
//!     &result,  // output
//! );
//! ```
//!
//! ## Replaying
//!
//! To replay an actor, use the `ReplayHandler`:
//!
//! ```ignore
//! let expected_chain = load_chain("actor_events.json")?;
//! let mut registry = HandlerRegistry::new();
//! registry.register(ReplayHandler::new(expected_chain));
//! ```

mod handler;

pub use handler::{ReplayHandler, ReplayState};

use serde::{Deserialize, Serialize};

/// A recorded host function call with full I/O.
///
/// This is the standardized event type for all handler host function calls.
/// It captures everything needed to replay the call: what function was called,
/// what inputs were provided, and what output was returned.
///
/// The type information comes from the component at replay time, so we only
/// need to store the serialized bytes here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionCall {
    /// The interface name (e.g., "theater:simple/timing", "wasi:clocks/wall-clock")
    pub interface: String,
    /// The function name (e.g., "now", "sleep")
    pub function: String,
    /// Serialized input parameters (JSON bytes)
    pub input: Vec<u8>,
    /// Serialized output/return value (JSON bytes)
    pub output: Vec<u8>,
}

impl HostFunctionCall {
    /// Create a new HostFunctionCall record.
    pub fn new(
        interface: impl Into<String>,
        function: impl Into<String>,
        input: Vec<u8>,
        output: Vec<u8>,
    ) -> Self {
        Self {
            interface: interface.into(),
            function: function.into(),
            input,
            output,
        }
    }

    /// Create a HostFunctionCall with no input parameters.
    pub fn no_input(
        interface: impl Into<String>,
        function: impl Into<String>,
        output: Vec<u8>,
    ) -> Self {
        Self::new(interface, function, vec![], output)
    }

    /// Create a HostFunctionCall by serializing input and output as JSON.
    pub fn from_io<I: Serialize, O: Serialize>(
        interface: impl Into<String>,
        function: impl Into<String>,
        input: &I,
        output: &O,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            interface: interface.into(),
            function: function.into(),
            input: serde_json::to_vec(input)?,
            output: serde_json::to_vec(output)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_function_call_serialization() {
        let call = HostFunctionCall::new(
            "theater:simple/timing",
            "now",
            vec![],
            serde_json::to_vec(&1234567890u64).unwrap(),
        );

        let json = serde_json::to_string(&call).unwrap();
        let parsed: HostFunctionCall = serde_json::from_str(&json).unwrap();

        assert_eq!(call.interface, parsed.interface);
        assert_eq!(call.function, parsed.function);
        assert_eq!(call.input, parsed.input);
        assert_eq!(call.output, parsed.output);
    }

    #[test]
    fn test_from_io() {
        let call = HostFunctionCall::from_io(
            "theater:simple/timing",
            "sleep",
            &1000u64,  // input: duration
            &(),       // output: unit (success)
        )
        .unwrap();

        assert_eq!(call.interface, "theater:simple/timing");
        assert_eq!(call.function, "sleep");

        // Verify we can deserialize the I/O
        let input: u64 = serde_json::from_slice(&call.input).unwrap();
        assert_eq!(input, 1000);
    }
}
