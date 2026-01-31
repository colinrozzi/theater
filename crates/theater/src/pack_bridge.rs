//! # Pack Bridge Module
//!
//! This module provides the integration layer between Theater and Pack.
//! It includes type conversions, wrapper types, and utilities for using
//! Pack's Graph ABI-based runtime within Theater's actor system.
//!
//! ## Key Components
//!
//! - **Re-exports**: Common Pack types for use throughout Theater
//! - **PackInstance**: Wrapper around a Pack instance with Theater integration
//! - **Value conversions**: Traits and implementations for converting between
//!   Pack's `Value` type and Theater's types

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;

// Re-export Pack types for convenient use throughout Theater
// Note: We use composite's internal abi module, not composite_abi crate directly,
// because that's what the runtime functions return.
pub use pack::abi::{Value, ValueType};
pub use pack::{
    AsyncCtx, AsyncInstance, AsyncRuntime, CallInterceptor, Ctx, HostFunctionProvider,
    HostLinkerBuilder, InterfaceBuilder, LinkerError,
};

use crate::actor::store::ActorStore;
use crate::id::TheaterId;

/// Metadata about an export function.
#[derive(Debug, Clone)]
pub struct ExportFunction {
    /// The interface name (e.g., "theater:simple/actor")
    pub interface: String,
    /// The function name (e.g., "init")
    pub function: String,
}

/// An instantiated Pack component with Theater integration.
///
/// This wraps Pack's `AsyncInstance` and provides methods for
/// calling functions and managing actor state.
///
/// ## Creation
///
/// Use `PackInstance::new()` to create an instance from WASM bytes:
///
/// ```ignore
/// let runtime = AsyncRuntime::new();
/// let instance = PackInstance::new(
///     "my-actor",
///     &wasm_bytes,
///     &runtime,
///     actor_store,
///     |builder| {
///         builder.interface("theater:simple/runtime")?
///             .func_typed("log", |ctx, msg: String| { ... })?;
///         Ok(())
///     }
/// ).await?;
/// ```
pub struct PackInstance {
    /// The actor name
    pub name: String,
    /// The underlying Pack instance
    pub instance: AsyncInstance<ActorStore>,
    /// The actor store
    pub actor_store: ActorStore,
    /// Registered export functions for this instance
    pub export_functions: HashMap<String, ExportFunction>,
}

impl PackInstance {
    /// Create a new Pack instance from WASM bytes.
    ///
    /// This loads and instantiates the module in one step, configuring
    /// host functions via the provided closure.
    ///
    /// ## Parameters
    ///
    /// * `name` - Name for this instance (typically the actor name)
    /// * `wasm_bytes` - The WASM binary to load
    /// * `runtime` - The async runtime to use
    /// * `actor_store` - The actor store containing state and communication channels
    /// * `configure` - A closure that configures host functions using the builder
    ///
    /// ## Returns
    ///
    /// A `PackInstance` ready for function calls.
    pub async fn new<F>(
        name: impl Into<String>,
        wasm_bytes: &[u8],
        runtime: &AsyncRuntime,
        actor_store: ActorStore,
        configure: F,
    ) -> Result<Self>
    where
        F: FnOnce(&mut HostLinkerBuilder<'_, ActorStore>) -> Result<(), LinkerError>,
    {
        Self::new_with_interceptor(name, wasm_bytes, runtime, actor_store, None, configure).await
    }

    /// Create a new Pack instance with an optional call interceptor.
    ///
    /// The interceptor is set on both the `HostLinkerBuilder` (to intercept
    /// import/host function calls) and on the resulting `AsyncInstance`
    /// (to intercept export/WASM function calls).
    pub async fn new_with_interceptor<F>(
        name: impl Into<String>,
        wasm_bytes: &[u8],
        runtime: &AsyncRuntime,
        actor_store: ActorStore,
        interceptor: Option<Arc<dyn CallInterceptor>>,
        configure: F,
    ) -> Result<Self>
    where
        F: FnOnce(&mut HostLinkerBuilder<'_, ActorStore>) -> Result<(), LinkerError>,
    {
        let module = runtime
            .load_module(wasm_bytes)
            .context("Failed to load WASM module with Pack runtime")?;

        let instance = module
            .instantiate_with_host_and_interceptor_async(
                actor_store.clone(),
                interceptor,
                configure,
            )
            .await
            .context("Failed to instantiate Pack module")?;

        Ok(Self {
            name: name.into(),
            instance,
            actor_store,
            export_functions: HashMap::new(),
        })
    }

    /// Get the actor ID from the store.
    pub fn id(&self) -> TheaterId {
        self.actor_store.id.clone()
    }

    /// Register an export function for later calling.
    ///
    /// This records metadata about an expected export function.
    pub fn register_export(&mut self, interface: &str, function: &str) {
        let key = format!("{}.{}", interface, function);
        self.export_functions.insert(
            key,
            ExportFunction {
                interface: interface.to_string(),
                function: function.to_string(),
            },
        );
    }

    /// Check if a function is registered.
    pub fn has_function(&self, name: &str) -> bool {
        self.export_functions.contains_key(name)
    }

    /// Call an export function with the given state and parameters.
    ///
    /// This is the primary way to invoke actor functions. It:
    /// 1. Validates the function is registered
    /// 2. Encodes the input as a Graph ABI value
    /// 3. Calls the function using the full qualified name
    /// 4. Decodes the output
    ///
    /// ## Parameters
    ///
    /// * `function_name` - The function name (e.g., "theater:simple/actor.init")
    /// * `state` - Current actor state (optional)
    /// * `params` - Parameters encoded as bytes (will be decoded and re-encoded as Value)
    ///
    /// ## Returns
    ///
    /// A tuple of (new_state, result_bytes).
    pub async fn call_function(
        &mut self,
        function_name: &str,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        // Validate the function is registered
        if !self.export_functions.contains_key(function_name) {
            return Err(anyhow::anyhow!("Function '{}' not registered", function_name));
        }

        // Build input value: tuple of (state, params)
        let state_value = state_to_value(state);
        let params_value = bytes_to_value(&params);
        let input = Value::Tuple(vec![state_value, params_value]);

        // Call the function using the full name (Pack exports use the #[export(name = "...")] syntax)
        let output = self
            .instance
            .call_with_value_async(function_name, &input)
            .await
            .context(format!("Failed to call function '{}'", function_name))?;

        // Decode the result
        decode_function_result(output)
    }

    /// Call a simple function that takes and returns a Value directly.
    ///
    /// This is useful for functions that don't follow the state pattern.
    pub async fn call_value(&mut self, function_name: &str, input: &Value) -> Result<Value> {
        self.instance
            .call_with_value_async(function_name, input)
            .await
            .context(format!("Failed to call function '{}'", function_name))
    }
}

// =============================================================================
// Value Conversion Utilities
// =============================================================================

/// Convert actor state to a Value.
fn state_to_value(state: Option<Vec<u8>>) -> Value {
    use pack::abi::ValueType;
    match state {
        Some(bytes) => Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: Some(Box::new(Value::List {
                elem_type: ValueType::U8,
                items: bytes.into_iter().map(Value::U8).collect(),
            })),
        },
        None => Value::Option {
            inner_type: ValueType::List(Box::new(ValueType::U8)),
            value: None,
        },
    }
}

/// Convert bytes to a Value (as a list of u8).
fn bytes_to_value(bytes: &[u8]) -> Value {
    use pack::abi::ValueType;
    Value::List {
        elem_type: ValueType::U8,
        items: bytes.iter().copied().map(Value::U8).collect(),
    }
}

/// Convert a Value (list of u8) back to bytes.
fn value_to_bytes(value: Value) -> Result<Vec<u8>> {
    match value {
        Value::List { items, .. } => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::U8(b) => bytes.push(b),
                    other => {
                        return Err(anyhow::anyhow!("Expected U8 in list, got {:?}", other))
                    }
                }
            }
            Ok(bytes)
        }
        other => Err(anyhow::anyhow!("Expected List, got {:?}", other)),
    }
}

/// Encode a Value to bytes using the Graph ABI.
pub fn encode_value(value: &Value) -> Result<Vec<u8>> {
    pack::encode(value).map_err(|e| anyhow::anyhow!("Failed to encode value: {:?}", e))
}

/// Decode bytes to a Value using the Graph ABI.
pub fn decode_value(bytes: &[u8]) -> Result<Value> {
    pack::decode(bytes).map_err(|e| anyhow::anyhow!("Failed to decode value: {:?}", e))
}

/// Decode a function result in the standard format.
///
/// Expected format: result<tuple<option<list<u8>>, R>, string>
/// Where R is the function-specific result type.
fn decode_function_result(value: Value) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
    match value {
        // Result variant: tag 0 = Ok, tag 1 = Err
        Value::Variant {
            tag: 0,
            payload,
            ..
        } if !payload.is_empty() => {
            // Ok case - payload is tuple<option<list<u8>>, R>
            match payload.into_iter().next().unwrap() {
                Value::Tuple(items) if !items.is_empty() => {
                    // First element is state
                    let new_state = match &items[0] {
                        Value::Option { value: Some(inner), .. } => Some(value_to_bytes((**inner).clone())?),
                        Value::Option { value: None, .. } => None,
                        other => {
                            return Err(anyhow::anyhow!(
                                "Expected Option for state, got {:?}",
                                other
                            ))
                        }
                    };

                    // Encode the rest as the result (or just the second element)
                    let result_value = if items.len() > 1 {
                        items[1].clone()
                    } else {
                        Value::Tuple(vec![])
                    };
                    let result_bytes = encode_value(&result_value)?;

                    Ok((new_state, result_bytes))
                }
                other => {
                    // Maybe it's just the state directly
                    let result_bytes = encode_value(&other)?;
                    Ok((None, result_bytes))
                }
            }
        }
        Value::Variant { tag: 0, payload, .. } if payload.is_empty() => {
            // Ok with no payload
            Ok((None, vec![]))
        }
        Value::Variant {
            tag: 1,
            payload,
            ..
        } if !payload.is_empty() => {
            // Err case - payload is the error message
            let error_msg = match payload.into_iter().next().unwrap() {
                Value::String(s) => s,
                other => format!("{:?}", other),
            };
            Err(anyhow::anyhow!("Function returned error: {}", error_msg))
        }
        Value::Variant { tag: 1, payload, .. } if payload.is_empty() => {
            Err(anyhow::anyhow!("Function returned error (no message)"))
        }
        Value::Variant { tag, .. } => {
            Err(anyhow::anyhow!("Unexpected result variant tag: {}", tag))
        }
        // If it's not a variant, treat the whole value as the result
        other => {
            let result_bytes = encode_value(&other)?;
            Ok((None, result_bytes))
        }
    }
}

// =============================================================================
// Trait Implementations for Theater Types
// =============================================================================

/// Trait for converting Theater types to Pack Values.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

/// Trait for converting Pack Values to Theater types.
pub trait FromValue: Sized {
    fn from_value(value: Value) -> Result<Self>;
}

// Implement for common types

impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Tuple(vec![])
    }
}

impl FromValue for () {
    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Tuple(items) if items.is_empty() => Ok(()),
            other => Err(anyhow::anyhow!("Expected unit tuple, got {:?}", other)),
        }
    }
}

// Result type conversion for WIT result<T, E>
impl<T: IntoValue, E: IntoValue> IntoValue for Result<T, E> {
    fn into_value(self) -> Value {
        match self {
            Ok(v) => Value::Variant {
                type_name: String::from("result"),
                case_name: String::from("ok"),
                tag: 0,
                payload: vec![v.into_value()],
            },
            Err(e) => Value::Variant {
                type_name: String::from("result"),
                case_name: String::from("err"),
                tag: 1,
                payload: vec![e.into_value()],
            },
        }
    }
}

// Implement IntoValue for primitives
impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::String(self.to_string())
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl IntoValue for i64 {
    fn into_value(self) -> Value {
        Value::S64(self)
    }
}

impl IntoValue for u64 {
    fn into_value(self) -> Value {
        Value::U64(self)
    }
}

impl IntoValue for Vec<u8> {
    fn into_value(self) -> Value {
        use pack::abi::ValueType;
        Value::List {
            elem_type: ValueType::U8,
            items: self.into_iter().map(Value::U8).collect(),
        }
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        let (inner_type, value) = match self {
            Some(v) => {
                let val = v.into_value();
                let ty = val.infer_type();
                (ty, Some(Box::new(val)))
            }
            None => {
                // For None, we don't have a value to infer from, use a placeholder
                use pack::abi::ValueType;
                (ValueType::Bool, None)
            }
        };
        Value::Option { inner_type, value }
    }
}

impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        let items: Vec<Value> = self.into_iter().map(|v| v.into_value()).collect();
        let elem_type = items.first().map(|v| v.infer_type()).unwrap_or_else(|| {
            use pack::abi::ValueType;
            ValueType::Bool // placeholder for empty lists
        });
        Value::List { elem_type, items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_to_value() {
        let state = Some(vec![1, 2, 3]);
        let value = state_to_value(state);

        match value {
            Value::Option { value: Some(inner), .. } => match *inner {
                Value::List { items, .. } => {
                    assert_eq!(items.len(), 3);
                }
                _ => panic!("Expected List"),
            },
            _ => panic!("Expected Option(Some(...))"),
        }
    }

    #[test]
    fn test_value_to_bytes() {
        use pack::abi::ValueType;
        let value = Value::List {
            elem_type: ValueType::U8,
            items: vec![Value::U8(1), Value::U8(2), Value::U8(3)],
        };
        let bytes = value_to_bytes(value).unwrap();
        assert_eq!(bytes, vec![1, 2, 3]);
    }

    #[test]
    fn test_into_value_result() {
        let ok: Result<String, String> = Ok("success".to_string());
        let value = ok.into_value();

        match value {
            Value::Variant {
                tag: 0,
                ref payload,
                ..
            } if !payload.is_empty() => {
                match &payload[0] {
                    Value::String(s) => assert_eq!(s, "success"),
                    _ => panic!("Expected String"),
                }
            },
            _ => panic!("Expected Ok variant"),
        }
    }
}
