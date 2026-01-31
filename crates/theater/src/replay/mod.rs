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
//! Recording now happens automatically via the `CallInterceptor` at the Pack runtime level.
//! All host function calls are intercepted and recorded to the actor's chain without
//! handlers needing manual recording code.
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

use pack::abi::Value;
use serde::{Deserialize, Serialize};

/// A recorded host function call with full I/O and type information.
///
/// This is the standardized event type for all handler host function calls.
/// It captures everything needed to replay the call: what function was called,
/// what inputs were provided, and what output was returned.
///
/// Type information is preserved in Pack's `Value` format, which derives
/// Serialize/Deserialize and is self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionCall {
    /// The interface name (e.g., "wasi:clocks/wall-clock@0.2.3", "wasi:random/random@0.2.3")
    pub interface: String,
    /// The function name (e.g., "now", "sleep")
    pub function: String,
    /// Input parameters as a Pack Value
    pub input: Value,
    /// Output/return value as a Pack Value
    pub output: Value,
}

impl HostFunctionCall {
    /// Create a new HostFunctionCall record.
    pub fn new(
        interface: impl Into<String>,
        function: impl Into<String>,
        input: Value,
        output: Value,
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
        output: Value,
    ) -> Self {
        Self::new(interface, function, Value::Tuple(vec![]), output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_function_call_serialization() {
        let call = HostFunctionCall::new(
            "wasi:clocks/wall-clock@0.2.3",
            "now",
            Value::Tuple(vec![]),
            Value::U64(1234567890),
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
            Value::Tuple(vec![]),
            Value::U64(42),
        );

        let json = serde_json::to_string(&call).unwrap();

        // The output should contain "U64" type tag
        assert!(json.contains("U64"), "Type tag should be in JSON: {}", json);
    }

    #[test]
    fn test_no_input() {
        let call = HostFunctionCall::no_input(
            "wasi:clocks/wall-clock@0.2.3",
            "now",
            Value::U64(999),
        );

        assert_eq!(call.input, Value::Tuple(vec![]));
    }
}
