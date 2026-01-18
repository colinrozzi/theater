//! # Composite Bridge Module
//!
//! This module provides the integration layer between Theater and Composite.
//! It includes type conversions, wrapper types, and utilities for using
//! Composite's Graph ABI-based runtime within Theater's actor system.
//!
//! ## Key Components
//!
//! - **Re-exports**: Common Composite types for use throughout Theater
//! - **CompositeInstance**: Wrapper around a Composite instance with Theater integration
//! - **Value conversions**: Traits and implementations for converting between
//!   Composite's `Value` type and Theater's types

use anyhow::{Context, Result};
use std::collections::HashMap;

// Re-export Composite types for convenient use throughout Theater
// Note: We use composite's internal abi module, not composite_abi crate directly,
// because that's what the runtime functions return.
pub use composite::abi::Value;
pub use composite::{
    AsyncCtx, AsyncInstance, AsyncRuntime, Ctx, HostFunctionProvider, HostLinkerBuilder,
    InterfaceBuilder, LinkerError,
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

/// An instantiated Composite component with Theater integration.
///
/// This wraps Composite's `AsyncInstance` and provides methods for
/// calling functions and managing actor state.
///
/// ## Creation
///
/// Use `CompositeInstance::new()` to create an instance from WASM bytes:
///
/// ```ignore
/// let runtime = AsyncRuntime::new();
/// let instance = CompositeInstance::new(
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
pub struct CompositeInstance {
    /// The actor name
    pub name: String,
    /// The underlying Composite instance
    pub instance: AsyncInstance<ActorStore>,
    /// The actor store
    pub actor_store: ActorStore,
    /// Registered export functions for this instance
    pub export_functions: HashMap<String, ExportFunction>,
}

impl CompositeInstance {
    /// Create a new Composite instance from WASM bytes.
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
    /// A `CompositeInstance` ready for function calls.
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
        let module = runtime
            .load_module(wasm_bytes)
            .context("Failed to load WASM module with Composite runtime")?;

        let instance = module
            .instantiate_with_host_async(actor_store.clone(), configure)
            .await
            .context("Failed to instantiate Composite module")?;

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
    /// 1. Encodes the input as a Graph ABI value
    /// 2. Calls the function
    /// 3. Decodes the output
    ///
    /// ## Parameters
    ///
    /// * `function_name` - The function to call (e.g., "init")
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
        // Build input value: tuple of (state, params)
        let state_value = state_to_value(state);
        let params_value = bytes_to_value(&params);
        let input = Value::Tuple(vec![state_value, params_value]);

        // Call the function
        let output = self
            .instance
            .call_with_value_async(function_name, &input, 0)
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
            .call_with_value_async(function_name, input, 0)
            .await
            .context(format!("Failed to call function '{}'", function_name))
    }
}

// =============================================================================
// Value Conversion Utilities
// =============================================================================

/// Convert actor state to a Value.
fn state_to_value(state: Option<Vec<u8>>) -> Value {
    match state {
        Some(bytes) => Value::Option(Some(Box::new(Value::List(
            bytes.into_iter().map(Value::U8).collect(),
        )))),
        None => Value::Option(None),
    }
}

/// Convert bytes to a Value (as a list of u8).
fn bytes_to_value(bytes: &[u8]) -> Value {
    Value::List(bytes.iter().copied().map(Value::U8).collect())
}

/// Convert a Value (list of u8) back to bytes.
fn value_to_bytes(value: Value) -> Result<Vec<u8>> {
    match value {
        Value::List(items) => {
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
    composite::encode(value).map_err(|e| anyhow::anyhow!("Failed to encode value: {:?}", e))
}

/// Decode bytes to a Value using the Graph ABI.
pub fn decode_value(bytes: &[u8]) -> Result<Value> {
    composite::decode(bytes).map_err(|e| anyhow::anyhow!("Failed to decode value: {:?}", e))
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
            payload: Some(inner),
        } => {
            // Ok case - payload is tuple<option<list<u8>>, R>
            match *inner {
                Value::Tuple(items) if !items.is_empty() => {
                    // First element is state
                    let new_state = match &items[0] {
                        Value::Option(Some(inner)) => Some(value_to_bytes((**inner).clone())?),
                        Value::Option(None) => None,
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
        Value::Variant { tag: 0, payload: None } => {
            // Ok with no payload
            Ok((None, vec![]))
        }
        Value::Variant {
            tag: 1,
            payload: Some(inner),
        } => {
            // Err case - payload is the error message
            let error_msg = match *inner {
                Value::String(s) => s,
                other => format!("{:?}", other),
            };
            Err(anyhow::anyhow!("Function returned error: {}", error_msg))
        }
        Value::Variant { tag: 1, payload: None } => {
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

/// Trait for converting Theater types to Composite Values.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

/// Trait for converting Composite Values to Theater types.
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
                tag: 0,
                payload: Some(Box::new(v.into_value())),
            },
            Err(e) => Value::Variant {
                tag: 1,
                payload: Some(Box::new(e.into_value())),
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
        Value::List(self.into_iter().map(Value::U8).collect())
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        Value::Option(self.map(|v| Box::new(v.into_value())))
    }
}

impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        Value::List(self.into_iter().map(|v| v.into_value()).collect())
    }
}

// =============================================================================
// Unified Instance Abstraction
// =============================================================================

/// Abstraction over different WASM runtime instances.
///
/// This enum allows ActorRuntime to work with either wasmtime (legacy)
/// or Composite (new) instances transparently.
pub enum UnifiedInstance {
    /// Legacy wasmtime Component Model instance
    Wasmtime(crate::wasm::ActorInstance),
    /// New Composite Graph ABI instance
    Composite(CompositeInstance),
}

impl UnifiedInstance {
    /// Get the actor ID from the underlying instance.
    pub fn id(&self) -> crate::id::TheaterId {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.id(),
            UnifiedInstance::Composite(instance) => instance.id(),
        }
    }

    /// Check if a function is registered.
    pub fn has_function(&self, name: &str) -> bool {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.has_function(name),
            UnifiedInstance::Composite(instance) => instance.has_function(name),
        }
    }

    /// Call a function on the instance.
    pub async fn call_function(
        &mut self,
        name: &str,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.call_function(name, state, params).await,
            UnifiedInstance::Composite(instance) => {
                instance.call_function(name, state, params).await
            }
        }
    }

    /// Get state from the store.
    pub fn get_state(&self) -> Option<Vec<u8>> {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.store.data().get_state(),
            UnifiedInstance::Composite(instance) => instance.actor_store.get_state(),
        }
    }

    /// Set state in the store.
    pub fn set_state(&mut self, state: Option<Vec<u8>>) {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.store.data_mut().set_state(state),
            UnifiedInstance::Composite(instance) => instance.actor_store.set_state(state),
        }
    }

    /// Get the event chain.
    pub fn get_chain(&self) -> Vec<crate::chain::ChainEvent> {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.store.data().get_chain(),
            UnifiedInstance::Composite(instance) => instance.actor_store.get_chain(),
        }
    }

    /// Save the event chain.
    pub fn save_chain(&self) -> Result<()> {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.save_chain(),
            UnifiedInstance::Composite(instance) => instance.actor_store.save_chain(),
        }
    }

    /// Record an event in the chain.
    pub fn record_event(
        &self,
        event_data: crate::events::ChainEventData,
    ) -> crate::chain::ChainEvent {
        match self {
            UnifiedInstance::Wasmtime(instance) => instance.store.data().record_event(event_data),
            UnifiedInstance::Composite(instance) => instance.actor_store.record_event(event_data),
        }
    }

    /// Get the underlying wasmtime ActorInstance if this is a wasmtime instance.
    ///
    /// This is for legacy code that needs direct access to wasmtime internals.
    /// New code should use the UnifiedInstance methods instead.
    pub fn as_wasmtime(&self) -> Option<&crate::wasm::ActorInstance> {
        match self {
            UnifiedInstance::Wasmtime(instance) => Some(instance),
            UnifiedInstance::Composite(_) => None,
        }
    }

    /// Get mutable access to the underlying wasmtime ActorInstance if this is a wasmtime instance.
    ///
    /// This is for legacy code that needs direct access to wasmtime internals.
    /// New code should use the UnifiedInstance methods instead.
    pub fn as_wasmtime_mut(&mut self) -> Option<&mut crate::wasm::ActorInstance> {
        match self {
            UnifiedInstance::Wasmtime(instance) => Some(instance),
            UnifiedInstance::Composite(_) => None,
        }
    }

    /// Get the underlying CompositeInstance if this is a composite instance.
    pub fn as_composite(&self) -> Option<&CompositeInstance> {
        match self {
            UnifiedInstance::Wasmtime(_) => None,
            UnifiedInstance::Composite(instance) => Some(instance),
        }
    }

    /// Get mutable access to the underlying CompositeInstance if this is a composite instance.
    pub fn as_composite_mut(&mut self) -> Option<&mut CompositeInstance> {
        match self {
            UnifiedInstance::Wasmtime(_) => None,
            UnifiedInstance::Composite(instance) => Some(instance),
        }
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
            Value::Option(Some(inner)) => match *inner {
                Value::List(items) => {
                    assert_eq!(items.len(), 3);
                }
                _ => panic!("Expected List"),
            },
            _ => panic!("Expected Option(Some(...))"),
        }
    }

    #[test]
    fn test_value_to_bytes() {
        let value = Value::List(vec![Value::U8(1), Value::U8(2), Value::U8(3)]);
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
                payload: Some(inner),
            } => match *inner {
                Value::String(s) => assert_eq!(s, "success"),
                _ => panic!("Expected String"),
            },
            _ => panic!("Expected Ok variant"),
        }
    }
}
