//! # Replay Module
//!
//! This module provides types and handlers for recording and replaying actor executions.
//! The key insight is that the chain + component binary contain everything needed for replay:
//!
//! - **Chain**: Records which functions were called and their I/O with full type information
//! - **Component**: The WebAssembly component to replay
//!
//! ## Recording
//!
//! During normal execution, handlers record host function calls using `HostFunctionCall`:
//!
//! ```ignore
//! use val_serde::IntoSerializableVal;
//!
//! ctx.data_mut().record_host_function_call(
//!     "wasi:random/random@0.2.3",
//!     "get-random-u64",
//!     ().into_serializable_val(),      // input
//!     result.into_serializable_val(),  // output
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
use val_serde::SerializableVal;

/// A recorded host function call with full I/O and type information.
///
/// This is the standardized event type for all handler host function calls.
/// It captures everything needed to replay the call: what function was called,
/// what inputs were provided, and what output was returned.
///
/// Type information is preserved in the `SerializableVal` format, making
/// the chain self-describing and independent of the component for replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionCall {
    /// The interface name (e.g., "wasi:clocks/wall-clock@0.2.3", "wasi:random/random@0.2.3")
    pub interface: String,
    /// The function name (e.g., "now", "sleep")
    pub function: String,
    /// Input parameters with full type information
    pub input: SerializableVal,
    /// Output/return value with full type information
    pub output: SerializableVal,
}

impl HostFunctionCall {
    /// Create a new HostFunctionCall record.
    pub fn new(
        interface: impl Into<String>,
        function: impl Into<String>,
        input: SerializableVal,
        output: SerializableVal,
    ) -> Self {
        Self {
            interface: interface.into(),
            function: function.into(),
            input,
            output,
        }
    }

    /// Create a HostFunctionCall with no input parameters (empty tuple).
    pub fn no_input(
        interface: impl Into<String>,
        function: impl Into<String>,
        output: SerializableVal,
    ) -> Self {
        Self::new(interface, function, SerializableVal::Tuple(vec![]), output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use val_serde::IntoSerializableVal;

    #[test]
    fn test_host_function_call_serialization() {
        let call = HostFunctionCall::new(
            "wasi:clocks/wall-clock@0.2.3",
            "now",
            ().into_serializable_val(),
            1234567890u64.into_serializable_val(),
        );

        let json = serde_json::to_string(&call).unwrap();
        let parsed: HostFunctionCall = serde_json::from_str(&json).unwrap();

        assert_eq!(call.interface, parsed.interface);
        assert_eq!(call.function, parsed.function);
        assert_eq!(call.input, parsed.input);
        assert_eq!(call.output, parsed.output);
    }

    #[test]
    fn test_type_preserved_in_json() {
        let call = HostFunctionCall::new(
            "wasi:random/random@0.2.3",
            "get-random-u64",
            ().into_serializable_val(),
            42u64.into_serializable_val(),
        );

        let json = serde_json::to_string(&call).unwrap();

        // The output should contain {"U64": 42} not just 42
        assert!(json.contains(r#""U64""#), "Type tag should be in JSON: {}", json);
    }

    #[test]
    fn test_no_input() {
        let call = HostFunctionCall::no_input(
            "wasi:clocks/wall-clock@0.2.3",
            "now",
            999u64.into_serializable_val(),
        );

        assert_eq!(call.input, SerializableVal::Tuple(vec![]));
    }
}
