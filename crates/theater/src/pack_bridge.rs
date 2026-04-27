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
use std::sync::Arc;

// Re-export Pack types for convenient use throughout Theater
// Now unified: pack re-exports from pack_abi, so Value/FromValue/ConversionError are consistent
pub use packr::abi::{ConversionError, FromValue, Value, ValueType};

pub use packr::{
    AsyncCtx, AsyncInstance, AsyncRuntime, CallInterceptor, Ctx, HostFunctionProvider,
    HostLinkerBuilder, InterfaceBuilder, LinkerError,
};
// Re-export metadata types for querying actor exports/imports
pub use packr::{
    compute_interface_hash, compute_interface_hashes, decode_metadata_with_hashes,
    encode_metadata_with_hashes, hash_type, validate_value_in_type_space, FunctionSignature,
    InterfaceHash, MetadataError, MetadataWithHashes, PackageMetadata, ParamSignature, TypeDesc,
    TypeValidationError,
};
// Re-export type system types for building metadata in tests
pub use packr::types::{Arena, Function, Param, Type, TypeDef};
// Re-export interface implementation types for handler interface declarations
pub use packr::{FuncSignature, InterfaceImpl, PackParams, PackType, TypeHash};
// Re-export pact parsing for loading interface definitions from .pact files
pub use packr::{parse_pact, PactInterface};

use std::collections::HashMap;

use crate::actor::store::ActorStore;
use crate::id::TheaterId;

/// Cached type information for a function's parameters and return types,
/// used for host-side contract enforcement.
#[derive(Debug, Clone)]
pub struct FunctionTypeInfo {
    /// The declared type for each parameter.
    pub param_types: Vec<Type>,
    /// The declared return types.
    pub result_types: Vec<Type>,
    /// Type definitions available for resolving Ref types.
    pub type_defs: Vec<TypeDef>,
}

/// Extract functions from an Arena by finding a child arena with the given name.
///
/// The Arena structure from `decode_metadata` is:
/// ```text
/// Arena("package")
/// ├── Arena("imports")
/// │   ├── Arena("interface1") → functions
/// │   └── Arena("interface2") → functions
/// └── Arena("exports")
///     ├── Arena("interface1") → functions
///     └── Arena("interface2") → functions
/// ```
///
/// Returns tuples of (interface_name, function).
fn extract_functions_from_arena(
    arena: &PackageMetadata,
    section: &str,
) -> Vec<(String, FunctionSignature)> {
    let mut result = Vec::new();

    // Find the child arena with the given name (e.g., "imports" or "exports")
    for child in &arena.children {
        if child.name == section {
            // Each child of this arena is an interface
            for interface_arena in &child.children {
                let interface_name = &interface_arena.name;
                for func in &interface_arena.functions {
                    result.push((interface_name.clone(), func.clone()));
                }
            }
        }
    }

    result
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
///
/// ## Export Discovery
///
/// Pack packages embed type metadata accessible via `get_metadata()`.
/// This provides full type signatures for all imports and exports,
/// eliminating the need for manual export registration.
pub struct PackInstance {
    /// The actor name
    pub name: String,
    /// The underlying Pack instance
    pub instance: AsyncInstance<ActorStore>,
    /// The actor store
    pub actor_store: ActorStore,
    /// Cached parameter type info per function name, for host-side validation.
    /// Populated after instantiation via `cache_function_types()`.
    function_types: HashMap<String, FunctionTypeInfo>,
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
            function_types: HashMap::new(),
        })
    }

    /// Get the actor ID from the store.
    pub fn id(&self) -> TheaterId {
        self.actor_store.id
    }

    /// Get the package metadata describing imports and exports.
    ///
    /// This calls the `__pack_types` export embedded in the WASM module
    /// to retrieve full type signatures for all imports and exports.
    /// Returns `Err(MetadataError::NotFound)` if the package doesn't
    /// export `__pack_types`.
    pub async fn get_metadata(&mut self) -> Result<PackageMetadata, MetadataError> {
        self.instance.types().await
    }

    /// Check if the package exports a function with the given name.
    ///
    /// This queries the embedded package metadata to check for the export.
    pub async fn has_export(
        &mut self,
        interface: &str,
        function: &str,
    ) -> Result<bool, MetadataError> {
        let exports = self.get_exports().await?;
        Ok(exports
            .iter()
            .any(|(iface, func)| iface == interface && func.name == function))
    }

    /// Get the list of exported functions with their full type signatures.
    ///
    /// Returns tuples of (interface_name, function).
    pub async fn get_exports(&mut self) -> Result<Vec<(String, FunctionSignature)>, MetadataError> {
        let metadata = self.get_metadata().await?;
        Ok(extract_functions_from_arena(&metadata, "exports"))
    }

    /// Get the list of imported functions with their full type signatures.
    ///
    /// Returns tuples of (interface_name, function).
    pub async fn get_imports(&mut self) -> Result<Vec<(String, FunctionSignature)>, MetadataError> {
        let metadata = self.get_metadata().await?;
        Ok(extract_functions_from_arena(&metadata, "imports"))
    }

    /// Get metadata with interface hashes for compatibility checking.
    ///
    /// Returns the full metadata along with computed Merkle-tree hashes
    /// for each imported and exported interface. These hashes enable
    /// O(1) compatibility checking between components and handlers.
    pub async fn get_metadata_with_hashes(&mut self) -> Result<MetadataWithHashes, MetadataError> {
        self.instance.types_with_hashes().await
    }

    /// Get interface hashes for all imported interfaces.
    ///
    /// Returns a list of (interface_name, hash) pairs that can be compared
    /// against handler interface hashes for compatibility checking.
    pub async fn get_import_hashes(&mut self) -> Result<Vec<InterfaceHash>, MetadataError> {
        let metadata = self.get_metadata_with_hashes().await?;
        Ok(metadata.import_hashes)
    }

    /// Get interface hashes for all exported interfaces.
    pub async fn get_export_hashes(&mut self) -> Result<Vec<InterfaceHash>, MetadataError> {
        let metadata = self.get_metadata_with_hashes().await?;
        Ok(metadata.export_hashes)
    }

    /// Cache function type information from the package metadata.
    ///
    /// This reads the metadata once and stores resolved parameter types
    /// for each exported function, enabling host-side type validation
    /// before crossing the WASM boundary.
    pub async fn cache_function_types(&mut self) -> Result<(), MetadataError> {
        let metadata = self.get_metadata().await?;
        let mut function_types = HashMap::new();

        // Walk the exports section of the arena
        for child in &metadata.children {
            if child.name == "exports" {
                for interface_arena in &child.children {
                    // Collect type defs from the interface level
                    let interface_types = &interface_arena.types;

                    for func in &interface_arena.functions {
                        let full_name = format!("{}.{}", interface_arena.name, func.name);

                        // Merge function-scoped and interface-scoped type defs
                        let mut all_types = interface_types.clone();
                        all_types.extend(func.types.clone());

                        function_types.insert(
                            full_name,
                            FunctionTypeInfo {
                                param_types: func.params.iter().map(|p| p.ty.clone()).collect(),
                                result_types: func.results.clone(),
                                type_defs: all_types,
                            },
                        );
                    }
                }
            }
        }

        self.function_types = function_types;
        Ok(())
    }

    /// Call an export function with the given state and parameters.
    ///
    /// This is the primary way to invoke actor functions. It:
    /// 1. Encodes the input as a Graph ABI value
    /// 2. Calls the function using the full qualified name
    /// 3. Decodes the output
    ///
    /// ## Parameters
    ///
    /// * `function_name` - The function name (e.g., "theater:simple/actor.init")
    /// * `state` - Current actor state as a Value (optional)
    /// * `params` - Parameters encoded as bytes (will be decoded and re-encoded as Value)
    ///
    /// ## Returns
    ///
    /// A tuple of (new_state, result_bytes).
    pub async fn call_function(
        &mut self,
        function_name: &str,
        state: Value,
        params: Vec<u8>,
    ) -> Result<(Value, Vec<u8>)> {
        let params_value = bytes_to_value(&params);
        self.call_function_with_value(function_name, state, params_value)
            .await
    }

    /// Call an export function with structured Value params (no bytes_to_value flattening).
    ///
    /// Unlike `call_function` which converts raw bytes to a flat list of u8,
    /// this method takes a structured `Value` directly, preserving the type
    /// information needed for Pack's Graph ABI encoding.
    pub async fn call_function_with_value(
        &mut self,
        function_name: &str,
        state: Value,
        params: Value,
    ) -> Result<(Value, Vec<u8>)> {
        // Validate state against the function's expected first parameter type.
        // We guarantee to actors that values match their declared types.
        if !self.function_types.is_empty() {
            let info = self.function_types.get(function_name).ok_or_else(|| {
                anyhow::anyhow!(
                    "Function '{}' not found in cached type metadata",
                    function_name
                )
            })?;
            if let Some(first_param) = info.param_types.first() {
                validate_value_in_type_space(&state, first_param, &info.type_defs).map_err(
                    |e| anyhow::anyhow!("State type mismatch for '{}': {}", function_name, e),
                )?;
            }
        }

        // Flatten: prepend state to params
        let input = match params {
            Value::Tuple(items) => {
                let mut all = Vec::with_capacity(1 + items.len());
                all.push(state);
                all.extend(items);
                Value::Tuple(all)
            }
            other => Value::Tuple(vec![state, other]),
        };

        let output = self
            .instance
            .call_with_value_async(function_name, &input)
            .await
            .context(format!("Failed to call function '{}'", function_name))?;

        // Validate return value against the function's declared result types.
        if !self.function_types.is_empty() {
            if let Some(info) = self.function_types.get(function_name) {
                if let Some(result_type) = info.result_types.first() {
                    validate_value_in_type_space(&output, result_type, &info.type_defs).map_err(
                        |e| {
                            anyhow::anyhow!("Return type violation from '{}': {}", function_name, e)
                        },
                    )?;
                }
            }
        }

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

    /// Call an actor function with typed decoding.
    ///
    /// This provides a higher-level API that:
    /// - Automatically wraps state and params into the expected tuple
    /// - Decodes the result into `ActorResult<T>` with the new state and typed return value
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result: ActorResult<i32> = instance.call_typed(
    ///     "increment",
    ///     state,
    ///     Value::Tuple(vec![]),
    /// ).await?;
    /// println!("New count: {}, state: {:?}", result.value, result.state);
    /// ```
    pub async fn call_typed<T: FromValue>(
        &mut self,
        function_name: &str,
        state: Value,
        params: Value,
    ) -> Result<ActorResult<T>> {
        let (new_state, result_bytes) = self
            .call_function_with_value(function_name, state, params)
            .await?;
        let result_value = if result_bytes.is_empty() {
            Value::Tuple(vec![]) // Unit when no return value
        } else {
            decode_value(&result_bytes)?
        };
        let value = T::from_value(result_value)
            .map_err(|e| anyhow::anyhow!("Failed to decode result: {:?}", e))?;
        Ok(ActorResult {
            state: new_state,
            value,
        })
    }
}

// =============================================================================
// Value Conversion Utilities
// =============================================================================

/// Convert bytes to a Value (as a list of u8).
fn bytes_to_value(bytes: &[u8]) -> Value {
    use packr::abi::ValueType;
    Value::List {
        elem_type: ValueType::U8,
        items: bytes.iter().copied().map(Value::U8).collect(),
    }
}

/// Encode a Value to bytes using the Graph ABI.
pub fn encode_value(value: &Value) -> Result<Vec<u8>> {
    packr::encode(value).map_err(|e| anyhow::anyhow!("Failed to encode value: {:?}", e))
}

/// Decode bytes to a Value using the Graph ABI.
pub fn decode_value(bytes: &[u8]) -> Result<Value> {
    packr::decode(bytes).map_err(|e| anyhow::anyhow!("Failed to decode value: {:?}", e))
}

/// Decode a function result in the standard format.
///
/// Expected format: result<tuple<option<state>, R>, string>
/// Where state is a Value and R is the function-specific result type.
fn decode_function_result(value: Value) -> Result<(Value, Vec<u8>)> {
    match value {
        // Handle Value::Result (Pack's native result type)
        Value::Result {
            value: Ok(inner), ..
        } => decode_ok_payload(*inner),
        Value::Result {
            value: Err(err), ..
        } => {
            let error_msg = match *err {
                Value::String(s) => s,
                other => format!("{:?}", other),
            };
            Err(anyhow::anyhow!("Function returned error: {}", error_msg))
        }
        // Handle Value::Variant (alternative encoding)
        Value::Variant {
            tag: 0, payload, ..
        } if !payload.is_empty() => decode_ok_payload(payload.into_iter().next().unwrap()),
        Value::Variant {
            tag: 1, payload, ..
        } if !payload.is_empty() => {
            let error_msg = match payload.into_iter().next().unwrap() {
                Value::String(s) => s,
                other => format!("{:?}", other),
            };
            Err(anyhow::anyhow!("Function returned error: {}", error_msg))
        }
        Value::Variant { tag: 1, .. } => {
            Err(anyhow::anyhow!("Function returned error (no message)"))
        }
        Value::Variant { tag, .. } => {
            Err(anyhow::anyhow!("Unexpected result variant tag: {}", tag))
        }
        // If it's not a variant or result, treat the whole value as the state
        other => Ok((other, vec![])),
    }
}

/// Helper to decode the Ok payload of a result.
/// Expected format: tuple<state, R...> where first element is state
fn decode_ok_payload(value: Value) -> Result<(Value, Vec<u8>)> {
    match value {
        Value::Tuple(mut items) if !items.is_empty() => {
            // First element is state directly
            let new_state = items.remove(0);

            // Remaining elements are the return value
            let result_value = if items.len() == 1 {
                items.remove(0)
            } else if items.is_empty() {
                Value::Tuple(vec![])
            } else {
                Value::Tuple(items)
            };
            let result_bytes = encode_value(&result_value)?;

            Ok((new_state, result_bytes))
        }
        // Single value — treat as state with no additional return
        other => Ok((other, vec![])),
    }
}

// =============================================================================
// Actor Result Type - for typed decoding of actor function results
// =============================================================================

/// Result type for actor function calls.
///
/// Theater actors return `result<tuple<option<state>, R>, string>` where:
/// - First tuple element is the updated state as a Value (or None)
/// - Second element is the function's return value
/// - Error case contains an error message
///
/// This type provides typed decoding via `FromValue`.
///
/// # Example
///
/// ```ignore
/// // Call an actor function with typed result
/// let result: ActorResult<i32> = instance.call_typed("increment", params).await?;
/// let new_state = result.state;
/// let count = result.value;
/// ```
#[derive(Debug, Clone)]
pub struct ActorResult<T> {
    /// The updated actor state
    pub state: Value,
    /// The function's return value
    pub value: T,
}

impl<T: FromValue> FromValue for ActorResult<T> {
    fn from_value(value: Value) -> std::result::Result<Self, ConversionError> {
        match value {
            // Handle Value::Result (Pack's native result type)
            Value::Result {
                value: Ok(inner), ..
            } => decode_actor_ok_payload(*inner),
            Value::Result {
                value: Err(err), ..
            } => {
                let error_msg = match *err {
                    Value::String(s) => s,
                    other => format!("{:?}", other),
                };
                Err(ConversionError::TypeMismatch {
                    expected: String::from("Ok result"),
                    got: format!("Actor error: {}", error_msg),
                })
            }
            // Handle Value::Variant (alternative encoding)
            Value::Variant {
                tag: 0, payload, ..
            } if !payload.is_empty() => {
                decode_actor_ok_payload(payload.into_iter().next().unwrap())
            }
            Value::Variant { tag: 0, .. } => Err(ConversionError::MissingPayload),
            Value::Variant {
                tag: 1, payload, ..
            } => {
                let error_msg = payload
                    .into_iter()
                    .next()
                    .map(|v| match v {
                        Value::String(s) => s,
                        other => format!("{:?}", other),
                    })
                    .unwrap_or_else(|| "Unknown error".to_string());
                Err(ConversionError::TypeMismatch {
                    expected: String::from("Ok result"),
                    got: format!("Actor error: {}", error_msg),
                })
            }
            other => Err(ConversionError::TypeMismatch {
                expected: String::from("Result or Variant"),
                got: format!("{:?}", other),
            }),
        }
    }
}

/// Helper to decode the Ok payload of an actor result.
fn decode_actor_ok_payload<T: FromValue>(
    value: Value,
) -> std::result::Result<ActorResult<T>, ConversionError> {
    match value {
        Value::Tuple(mut items) if !items.is_empty() => {
            // First element is state
            let state = items.remove(0);

            // Remaining elements are the return value
            let return_value = if items.len() == 1 {
                items.remove(0)
            } else if items.is_empty() {
                Value::Tuple(vec![])
            } else {
                Value::Tuple(items)
            };

            let value = T::from_value(return_value)?;
            Ok(ActorResult { state, value })
        }
        // Single value — treat as state with no return
        other => Ok(ActorResult {
            state: other,
            value: T::from_value(Value::Tuple(vec![]))?,
        }),
    }
}

// =============================================================================
// Trait Implementations for Theater Types
// =============================================================================

/// Trait for converting Theater types to Pack Values.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

// Note: FromValue is now imported from Pack (packr::abi::FromValue)

// Implement for common types

impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Tuple(vec![])
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
        use packr::abi::ValueType;
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
                use packr::abi::ValueType;
                (ValueType::Bool, None)
            }
        };
        Value::Option { inner_type, value }
    }
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        let items: Vec<Value> = self.into_iter().map(|v| v.into_value()).collect();
        let elem_type = items.first().map(|v| v.infer_type()).unwrap_or_else(|| {
            use packr::abi::ValueType;
            ValueType::Bool // placeholder for empty lists
        });
        Value::List { elem_type, items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_into_value_result() {
        let ok: Result<String, String> = Ok("success".to_string());
        let value = ok.into_value();

        match value {
            Value::Variant {
                tag: 0,
                ref payload,
                ..
            } if !payload.is_empty() => match &payload[0] {
                Value::String(s) => assert_eq!(s, "success"),
                _ => panic!("Expected String"),
            },
            _ => panic!("Expected Ok variant"),
        }
    }
}
