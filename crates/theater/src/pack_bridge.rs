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
use std::time::Duration;

/// How often the epoch ticker advances the shared engine's epoch. Guest
/// deadlines are expressed in ticks, so with a 1s tick, N ticks ~= N seconds.
const EPOCH_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Per-call epoch deadline for `actor.init` (ticks ~= seconds). Init is the
/// spine-wedging path (a runaway init sticks the parent's synchronous spawn),
/// so it gets a tight ceiling — the init-watchdog warns at 30s, epoch traps here.
const INIT_EPOCH_DEADLINE_TICKS: u64 = 60;

/// Per-call epoch deadline for all other guest calls (ticks ~= seconds). A
/// generous hard ceiling: legit calls are milliseconds (a 1MB decode is ~4ms),
/// so this never false-trips but stops a true runaway from pegging a core.
const DEFAULT_EPOCH_DEADLINE_TICKS: u64 = 300;

// Re-export Pack types for convenient use throughout Theater
// Now unified: pack re-exports from pack_abi, so Value/FromValue/ConversionError are consistent
pub use packr::abi::{ConversionError, FromValue, Value, ValueType};
pub use packr_abi::{GraphValue, Pattern};

pub use packr::{
    AsyncCompiledModule, AsyncCtx, AsyncInstance, AsyncRuntime, CallInterceptor, Ctx,
    HostFunctionProvider, HostLinkerBuilder, InterfaceBuilder, LinkerError, Module,
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

/// Shared wasm runtime with an engine-scoped compile cache.
///
/// Wraps one `AsyncRuntime` (one `wasmtime::Engine`) plus a map from
/// content hash of wasm bytes to the compiled `Module`. Spawning N actors
/// from the same wasm pays the cranelift compile cost once; subsequent
/// spawns wrap the cached module and skip straight to instantiation.
///
/// The cache key is the SHA-256 of the raw wasm bytes, so invalidation is
/// a non-issue: different bytes are a different entry, identical bytes are
/// identical modules. Entries live for the lifetime of this runtime
/// (in a theater server, the process lifetime).
///
/// Cache and engine are deliberately one struct: a `wasmtime::Module` is
/// engine-scoped, so a cache keyed only by content hash but shared across
/// engines would hand out modules that fail instantiation. Owning both
/// makes that misuse unrepresentable.
pub struct CachingPackRuntime {
    runtime: AsyncRuntime,
    modules: std::sync::RwLock<HashMap<[u8; 32], Module>>,
}

impl CachingPackRuntime {
    pub fn new() -> Self {
        let runtime = AsyncRuntime::new();

        // Epoch ticker: advance the shared engine's epoch once per second so a
        // per-call `set_epoch_deadline` can trap a runaway guest (a decode, a
        // loop, anything) instead of letting it peg a core forever. One ticker
        // for the singleton engine. Guarded on Handle::try_current so building
        // the runtime outside a tokio context (e.g. a sync unit test) doesn't
        // panic — without a ticker the epoch never advances, so no call traps,
        // which is the correct behavior for a non-async harness.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let engine = runtime.engine().clone();
            handle.spawn(async move {
                let mut ticker = tokio::time::interval(EPOCH_TICK_INTERVAL);
                loop {
                    ticker.tick().await;
                    engine.increment_epoch();
                }
            });
        }

        Self {
            runtime,
            modules: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Get-or-compile the module for these bytes.
    ///
    /// Returns the wrapped compiled module and whether it was a cache hit.
    ///
    /// Concurrent misses on the same bytes both compile and the last
    /// insert wins — benign (both modules are valid for this engine) and
    /// preferable to holding the write lock across a multi-millisecond
    /// cranelift run. For today's stable actor set this is fine because
    /// cold cache only happens at process start; if burst-on-cold-cache
    /// ever matters (e.g. an inbound connection storm against a fresh
    /// host paying N × cranelift instead of 1), the mitigation is to
    /// change the value type to `Arc<OnceCell<Module>>` so concurrent
    /// misses on the same bytes share one compile.
    pub fn load_module_cached(&self, wasm_bytes: &[u8]) -> Result<(AsyncCompiledModule<'_>, bool)> {
        use sha2::{Digest, Sha256};
        let hash: [u8; 32] = Sha256::digest(wasm_bytes).into();

        if let Some(module) = self
            .modules
            .read()
            .expect("module cache lock poisoned")
            .get(&hash)
        {
            return Ok((self.runtime.wrap_module(module.clone()), true));
        }

        let compiled = self
            .runtime
            .load_module(wasm_bytes)
            .context("Failed to load WASM module with Pack runtime")?;
        self.modules
            .write()
            .expect("module cache lock poisoned")
            .insert(hash, compiled.module().clone());
        Ok((compiled, false))
    }

    /// The underlying runtime, for callers that need the uncached path
    /// or direct engine access.
    pub fn runtime(&self) -> &AsyncRuntime {
        &self.runtime
    }

    /// Number of distinct modules currently cached.
    pub fn cached_module_count(&self) -> usize {
        self.modules
            .read()
            .expect("module cache lock poisoned")
            .len()
    }
}

impl Default for CachingPackRuntime {
    fn default() -> Self {
        Self::new()
    }
}

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
///         builder.interface("theater:simple/self")?
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

    /// Like [`Self::new_with_interceptor`], but compiles through the
    /// runtime's module cache: spawning the same wasm bytes repeatedly
    /// pays the cranelift compile once and instantiates from the cached
    /// module afterwards.
    pub async fn new_with_interceptor_cached<F>(
        name: impl Into<String>,
        wasm_bytes: &[u8],
        runtime: &CachingPackRuntime,
        actor_store: ActorStore,
        interceptor: Option<Arc<dyn CallInterceptor>>,
        configure: F,
    ) -> Result<Self>
    where
        F: FnOnce(&mut HostLinkerBuilder<'_, ActorStore>) -> Result<(), LinkerError>,
    {
        let (module, _cache_hit) = runtime.load_module_cached(wasm_bytes)?;

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

    /// Call an export function with the given parameters.
    ///
    /// This is the primary way to invoke actor functions. It:
    /// 1. Encodes the input as a Graph ABI value
    /// 2. Calls the function using the full qualified name
    /// 3. Decodes the output
    ///
    /// Actor state lives *inside* the module (see `docs/in-module-state.md`): the
    /// runtime never threads it through the call, so only the function's own
    /// parameters cross the boundary and only its own return comes back.
    ///
    /// ## Parameters
    ///
    /// * `function_name` - The function name (e.g., "theater:simple/actor.init")
    /// * `params` - Parameters encoded as bytes (will be decoded and re-encoded as Value)
    ///
    /// ## Returns
    ///
    /// The function's result encoded as bytes.
    pub async fn call_function(&mut self, function_name: &str, params: Vec<u8>) -> Result<Vec<u8>> {
        let params_value = bytes_to_value(&params);
        self.call_function_with_value(function_name, params_value)
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
        params: Value,
    ) -> Result<Vec<u8>> {
        // The guest export receives its parameters as a tuple; make sure a bare
        // value is wrapped (a `Tuple` is passed through as-is). Nothing is
        // prepended — state is the module's own, not the runtime's.
        let input = match params {
            t @ Value::Tuple(_) => t,
            other => Value::Tuple(vec![other]),
        };

        // Arm the epoch deadline before entering the guest: with the 1/sec
        // ticker above, a runaway call traps once `ticks` seconds pass and
        // returns Err (an epoch trap) instead of pegging a core. Tight on
        // actor.init (the spine-wedging path), generous otherwise. The deadline
        // is per-call, so a legitimately slow call just needs a bigger budget.
        let epoch_ticks = if function_name == "theater:simple/actor.init" {
            INIT_EPOCH_DEADLINE_TICKS
        } else {
            DEFAULT_EPOCH_DEADLINE_TICKS
        };
        // Armed on packr >=0.10.6: pack-dev's u64::MAX "never-trap" default
        // (which overflowed current_epoch()+delta once the ticker advanced the
        // epoch past 0) is fixed to u64::MAX/2, so this computes cleanly and
        // traps a runaway call at the deadline instead of pegging a core.
        self.instance.set_epoch_deadline(epoch_ticks);

        // Diagnostic: capture the EXACT encoded actor.init input (the
        // Tuple[..params] the guest's composite_abi decoder receives) so a
        // hanging/pathological decode input can be handed to packr verbatim. This
        // is the real wasm-boundary input — the actor's init config (from the
        // manifest's initial_state) is delivered here as the init argument.
        // Off by default; the hex-encode (and re-encode) only run when enabled:
        //   RUST_LOG=theater::init_encode_dump=trace
        if function_name == "theater:simple/actor.init" {
            tracing::trace!(
                target: "theater::init_encode_dump",
                hex = %hex::encode(encode_value(&input).unwrap_or_default()),
                "actor.init encoded input bytes (packr encode of Tuple[..params])"
            );
        }

        let output = self
            .instance
            .call_with_value_async(function_name, &input)
            .await
            .with_context(|| {
                format!(
                    "Failed to call function '{}' with input: {}",
                    function_name, input
                )
            })?;

        // Validate the return against the function's declared result type — with
        // one exception: an `ok(())` (unit ok). A `result<_, E>` return carries no
        // ok payload to type-check, and packr's metadata surfaces that empty
        // ok-type to the validator as `bool`, so validating a unit ok spuriously
        // fails ("expected bool, got tuple<0>"). A unit ok has no data to get
        // wrong, so it always conforms — skip it. Typed and error returns still
        // validate.
        let is_unit_ok = matches!(
            &output,
            Value::Result { value: Ok(inner), .. }
                if matches!(inner.as_ref(), Value::Tuple(items) if items.is_empty())
        );
        if !is_unit_ok && !self.function_types.is_empty() {
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

/// Decode a function result.
///
/// Expected format: `result<R, string>` where `R` is the function's own return
/// (no threaded state). The Ok payload *is* the return value; we encode it to
/// bytes for the caller.
fn decode_function_result(value: Value) -> Result<Vec<u8>> {
    match value {
        // Handle Value::Result (Pack's native result type)
        Value::Result {
            value: Ok(inner), ..
        } => encode_value(&inner),
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
        } if !payload.is_empty() => encode_value(&payload.into_iter().next().unwrap()),
        // Ok with no payload = unit return
        Value::Variant { tag: 0, .. } => Ok(vec![]),
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
        // Not a variant or result — treat the whole value as the return.
        other => encode_value(&other),
    }
}

// =============================================================================
// Trait Implementations for Theater Types
// =============================================================================

/// Trait for converting Theater types to Pack Values.
// Type→Value conversion is packr's job now: primitives, `Option<T>`, `Vec<T>`,
// etc. impl `From<T> for Value` in packr-abi, and domain types derive it with
// `#[derive(GraphValue)]`. Use `Value::from(x)` / `x.into()` rather than a
// theater-local trait.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_cache_hits_on_identical_bytes() {
        let rt = CachingPackRuntime::new();
        // wasmtime's default `wat` feature accepts text modules.
        let wat_a = b"(module)";
        let wat_b = b"(module (func))";

        let (_, hit) = rt.load_module_cached(wat_a).unwrap();
        assert!(!hit, "first load of A must be a miss");
        assert_eq!(rt.cached_module_count(), 1);

        let (_, hit) = rt.load_module_cached(wat_a).unwrap();
        assert!(hit, "second load of A must be a hit");
        assert_eq!(rt.cached_module_count(), 1);

        let (_, hit) = rt.load_module_cached(wat_b).unwrap();
        assert!(!hit, "different bytes must be a miss");
        assert_eq!(rt.cached_module_count(), 2);
    }
}
