//! # Pack Bridge Module
//!
//! This module provides the integration layer between Theater and Pack.
//! It includes type conversions, wrapper types, and utilities for using
//! Pack's Graph ABI-based runtime within Theater's actor system.
//!
//! ## Engine axis (packr-core 0.24)
//!
//! Theater drives wasm actors through the **capture-based** packr-core engine:
//! [`packr_wasmtime::WasmtimeEngine`] compiles + instantiates modules, host
//! functions are registered on a [`packr_core::HostImports`] (each closure
//! captures its own state — no typed store is threaded through the engine), and
//! exports are called through [`packr_core::call_with_value`]. See
//! `docs/engine-axis.md`.
//!
//! ## Key Components
//!
//! - **Re-exports**: Common Pack types for use throughout Theater
//! - **PackInstance**: Wrapper around a Pack instance with Theater integration
//! - **CachingPackRuntime**: the shared engine + compile cache
//! - **InterfaceImpl**: handler interface declaration + content-addressed
//!   hashing (single-sourced with the actor read side via packr-core primitives)

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use packr_core::backend::{WasmEngine, WasmInstance};
use packr_wasmtime::WasmtimeEngine;

use crate::actor::store::ActorStore;
use crate::id::TheaterId;

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

/// The concrete compiled-module handle for the native backend.
type CachedModule = <WasmtimeEngine as WasmEngine>::Module;
/// The concrete live-instance type for the native backend.
type EngineInstance = <WasmtimeEngine as WasmEngine>::Instance;

// =============================================================================
// Re-exports
// =============================================================================

// The graph ABI value types — single-sourced from packr-abi (packr_core::abi is
// the same crate), so theater/handlers see exactly one `Value`.
pub use packr_abi::{GraphValue, Pattern};
pub use packr_core::abi::{ConversionError, FromValue, Value, ValueType};

// The capture-based host-import surface + the record/replay interceptor trait.
pub use packr_core::{host_fn, CallInterceptor, HostError, HostFn, HostImports};

// Content-addressed metadata read from the module's embedded CGRF section, plus
// the interface-hash primitives.
pub use packr_core::metadata::{
    compute_interface_hash, compute_interface_hashes, metadata_with_hashes_from_module,
    InterfaceHash, MetadataError, MetadataWithHashes,
};

// The pact type-system AST.
pub use packr_abi::types::{Arena, Case, Field, Function, Param, Type, TypeDef, TypePath};
pub use packr_abi::TypeHash;

// The `.pact` TEXT parser. packr-core 0.24 ships the type-system AST + the hash
// primitives but NOT a pact-source parser, so the parser is still sourced from
// the umbrella `packr` crate (its AST is `packr_abi::types`, so the parsed
// interface feeds straight into the packr-core hashing below). See the
// engine-axis migration notes.
pub use packr::{parse_pact, MetadataValue, PactExport, PactInterface};

/// Adapter restoring `func_async_result` ergonomics on packr-core's raw `host_fn`.
///
/// The capture-based `host_fn` returns a raw `Result<Value, HostError>` with no
/// auto-wrapping; a pact `result<ok, err>` return must be built as a
/// `Value::Result` explicitly. This wraps a closure returning `Result<Value,
/// Value>` (pact ok / pact err) into that `Value::Result`. The guest's typed
/// decode ignores `ok_type`/`err_type` (it dispatches on the Ok/Err tag +
/// payload), so the declared types only need to be valid and deterministic — we
/// pass them from the pact signature at registration so recorded values stay
/// replay-faithful. A closure `Err(HostError)` is a genuine host-side failure
/// (dispatch status -1), distinct from a pact err (`Ok(Err(v))`).
pub fn result_host_fn<F, Fut>(f: F) -> packr_core::HostFn
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<
            Output = Result<std::result::Result<Value, Value>, packr_core::HostError>,
        > + Send
        + 'static,
{
    packr_core::host_fn(move |input| {
        let fut = f(input);
        async move {
            let payload = fut.await?;
            // The guest's typed decode dispatches on the Ok/Err tag + payload and
            // *ignores* the result's declared `ok_type`/`err_type`; replay
            // short-circuits to the recorded value, so live and replay use the
            // same types either way. We infer the PRESENT branch's type from its
            // value (matches by construction → encodes cleanly) and use unit as a
            // valid, deterministic placeholder for the ABSENT branch. Preserving
            // the pact signature's alias names for the stored types is a possible
            // faithfulness follow-up, not a correctness requirement.
            let unit = packr_core::abi::ValueType::Tuple(Vec::new());
            let result = match payload {
                Ok(v) => Value::Result {
                    ok_type: v.infer_type(),
                    err_type: unit,
                    value: Ok(Box::new(v)),
                },
                Err(v) => Value::Result {
                    ok_type: unit,
                    err_type: v.infer_type(),
                    value: Err(Box::new(v)),
                },
            };
            Ok(result)
        }
    })
}

/// Like [`result_host_fn`] but for closures that cannot fail at the HOST level:
/// the closure returns `Result<Value, Value>` (pact ok / pact err) directly, and
/// this lifts it into the `Ok(..)` (no [`HostError`]) the engine expects.
///
/// This is the drop-in for the old `func_async_result` host functions, whose
/// bodies already return `Ok::<Value, Value>(..)` for a pact ok and
/// `Err(pact_err_value)` for a pact err — so the body transfers verbatim; only
/// the captured state changes (no `ctx`). A genuine host trap is not expressible
/// here (there was none in the old bodies); use [`result_host_fn`] directly if a
/// host-level `Err(HostError)` is ever needed.
pub fn pact_result_host_fn<F, Fut>(f: F) -> packr_core::HostFn
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = std::result::Result<Value, Value>> + Send + 'static,
{
    result_host_fn(move |input| {
        let fut = f(input);
        async move { Ok(fut.await) }
    })
}

/// Wrap a closure returning a PLAIN [`Value`] (a non-`result` pact return) as a
/// [`HostFn`]. The drop-in for the old `func_typed` / `func_async` host
/// functions, whose bodies produce a `Value` directly.
pub fn plain_host_fn<F, Fut>(f: F) -> packr_core::HostFn
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Value> + Send + 'static,
{
    packr_core::host_fn(move |input| {
        let fut = f(input);
        async move { Ok(fut.await) }
    })
}

/// Shared wasm engine with an engine-scoped compile cache.
///
/// Wraps one [`WasmtimeEngine`] (one `wasmtime::Engine`) plus a map from content
/// hash of wasm bytes to the compiled `Module`. Spawning N actors from the same
/// wasm pays the cranelift compile cost once; subsequent spawns instantiate the
/// cached module directly.
///
/// The cache key is the SHA-256 of the raw wasm bytes, so invalidation is a
/// non-issue: different bytes are a different entry, identical bytes are
/// identical modules. Entries live for the lifetime of this runtime.
///
/// Cache and engine are deliberately one struct: a `wasmtime::Module` is
/// engine-scoped, so a cache keyed only by content hash but shared across
/// engines would hand out modules that fail instantiation. Owning both makes
/// that misuse unrepresentable.
pub struct CachingPackRuntime {
    engine: WasmtimeEngine,
    modules: std::sync::RwLock<HashMap<[u8; 32], CachedModule>>,
}

impl CachingPackRuntime {
    pub fn new() -> Self {
        let engine = WasmtimeEngine::new();

        // Epoch ticker: advance the shared engine's epoch once per second so a
        // per-call `set_deadline` can trap a runaway guest (a decode, a loop,
        // anything) instead of letting it peg a core forever. One ticker for the
        // singleton engine. Guarded on Handle::try_current so building the
        // runtime outside a tokio context (e.g. a sync unit test) doesn't panic —
        // without a ticker the epoch never advances, so no call traps, which is
        // the correct behavior for a non-async harness.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let wt_engine = engine.engine().clone();
            handle.spawn(async move {
                let mut ticker = tokio::time::interval(EPOCH_TICK_INTERVAL);
                loop {
                    ticker.tick().await;
                    wt_engine.increment_epoch();
                }
            });
        }

        Self {
            engine,
            modules: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Get-or-compile the module for these bytes.
    ///
    /// Returns the compiled module and whether it was a cache hit.
    ///
    /// Concurrent misses on the same bytes both compile and the last insert
    /// wins — benign (both modules are valid for this engine) and preferable to
    /// holding the write lock across a multi-millisecond cranelift run.
    pub async fn compile_cached(&self, wasm_bytes: &[u8]) -> Result<(CachedModule, bool)> {
        use sha2::{Digest, Sha256};
        let hash: [u8; 32] = Sha256::digest(wasm_bytes).into();

        if let Some(module) = self
            .modules
            .read()
            .expect("module cache lock poisoned")
            .get(&hash)
        {
            return Ok((module.clone(), true));
        }

        let module = self
            .engine
            .compile(wasm_bytes)
            .await
            .context("Failed to compile WASM module with Pack engine")?;
        self.modules
            .write()
            .expect("module cache lock poisoned")
            .insert(hash, module.clone());
        Ok((module, false))
    }

    /// The underlying engine, for instantiation and direct engine access.
    pub fn engine(&self) -> &WasmtimeEngine {
        &self.engine
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

/// An instantiated Pack component with Theater integration.
///
/// Wraps a live packr-core [`WasmInstance`] and provides the call + metadata
/// surface the runtime drives. Metadata (interface hashes, export presence) is
/// served **statically** from the module bytes — no live `__pack_types` call is
/// needed. The `actor_store` is retained for chain event recording + the actor
/// id; it is NOT threaded through the engine (host functions capture their own
/// state).
pub struct PackInstance {
    /// The actor name
    pub name: String,
    /// The underlying packr-core instance
    pub instance: EngineInstance,
    /// The actor store — used for chain event recording (`record_event`) and the
    /// actor id. Not passed to the engine (capture-based host-import model).
    pub actor_store: ActorStore,
    /// The raw wasm bytes, kept so metadata can be served statically.
    wasm_bytes: Arc<Vec<u8>>,
}

impl PackInstance {
    /// Wrap a freshly instantiated engine instance for Theater use.
    pub fn new(
        name: impl Into<String>,
        instance: EngineInstance,
        actor_store: ActorStore,
        wasm_bytes: Arc<Vec<u8>>,
    ) -> Self {
        Self {
            name: name.into(),
            instance,
            actor_store,
            wasm_bytes,
        }
    }

    /// Get the actor ID from the store.
    pub fn id(&self) -> TheaterId {
        self.actor_store.id
    }

    /// Get metadata with interface hashes for compatibility checking.
    ///
    /// Two sources, in order:
    /// 1. the module's embedded CGRF data segment (static, no guest call) — the
    ///    packr-guest 0.24 convention;
    /// 2. the guest's `__pack_types` export (a runtime call) — the packr-guest
    ///    0.23 convention, where the CGRF blob lives in general rodata behind a
    ///    callable accessor rather than a dedicated data segment.
    ///
    /// Returns [`MetadataError::NotFound`] if neither is present.
    pub async fn get_metadata_with_hashes(&mut self) -> Result<MetadataWithHashes, MetadataError> {
        if let Some(md) = metadata_with_hashes_from_module(&self.wasm_bytes)? {
            return Ok(md);
        }
        self.metadata_via_export().await
    }

    /// Call the guest's `__pack_types` export to fetch its CGRF metadata.
    ///
    /// Convention (unchanged from the umbrella runtime): the export has signature
    /// `(out_ptr_slot, out_len_slot) -> status`; it writes the `(ptr, len)` of
    /// the CGRF blob into the two guest-memory slots and returns 0 on success.
    async fn metadata_via_export(&mut self) -> Result<MetadataWithHashes, MetadataError> {
        use packr_core::{Val, RESULT_LEN_OFFSET, RESULT_PTR_OFFSET};

        if !self.instance.has_export("__pack_types") {
            return Err(MetadataError::NotFound);
        }

        let results = self
            .instance
            .call(
                "__pack_types",
                &[
                    Val::I32(RESULT_PTR_OFFSET as i32),
                    Val::I32(RESULT_LEN_OFFSET as i32),
                ],
            )
            .await
            .map_err(|e| MetadataError::CallFailed(e.to_string()))?;
        let status = results.first().copied().and_then(Val::as_i32).unwrap_or(-1);
        if status != 0 {
            return Err(MetadataError::CallFailed(
                "non-zero status from __pack_types".into(),
            ));
        }

        let mut ptr_bytes = [0u8; 4];
        let mut len_bytes = [0u8; 4];
        self.instance
            .read_memory(RESULT_PTR_OFFSET, &mut ptr_bytes)
            .map_err(|e| MetadataError::CallFailed(e.to_string()))?;
        self.instance
            .read_memory(RESULT_LEN_OFFSET, &mut len_bytes)
            .map_err(|e| MetadataError::CallFailed(e.to_string()))?;
        let out_ptr = u32::from_le_bytes(ptr_bytes) as usize;
        let out_len = u32::from_le_bytes(len_bytes) as usize;

        // `out_len` comes straight from guest memory, so cap it before allocating:
        // a broken or hostile 0.23 actor returning a bogus length must not force a
        // huge allocation ahead of any sanity check. `__pack_types` metadata is
        // embedded rodata (kilobytes in practice), so a generous ceiling never
        // bites a legitimate actor.
        const MAX_METADATA_LEN: usize = 64 * 1024 * 1024;
        if out_len > MAX_METADATA_LEN {
            return Err(MetadataError::CallFailed(format!(
                "__pack_types metadata length {out_len} exceeds the {MAX_METADATA_LEN}-byte cap"
            )));
        }

        // Static rodata — no `__pack_free` needed.
        let mut bytes = vec![0u8; out_len];
        self.instance
            .read_memory(out_ptr, &mut bytes)
            .map_err(|e| MetadataError::CallFailed(e.to_string()))?;

        packr_core::metadata::decode_metadata_with_hashes(&bytes)
    }

    /// Get interface hashes for all imported interfaces.
    pub async fn get_import_hashes(&mut self) -> Result<Vec<InterfaceHash>, MetadataError> {
        Ok(self.get_metadata_with_hashes().await?.import_hashes)
    }

    /// Get interface hashes for all exported interfaces.
    pub async fn get_export_hashes(&mut self) -> Result<Vec<InterfaceHash>, MetadataError> {
        Ok(self.get_metadata_with_hashes().await?.export_hashes)
    }

    /// Check if the package exports a function under the given interface.
    pub async fn has_export(
        &mut self,
        interface: &str,
        function: &str,
    ) -> Result<bool, MetadataError> {
        let metadata = self.get_metadata_with_hashes().await?;
        Ok(metadata
            .arena
            .exported_function_names(interface)
            .iter()
            .any(|f| f == function))
    }

    /// Call an export function with raw ABI-encoded params.
    pub async fn call_function(&mut self, function_name: &str, params: Vec<u8>) -> Result<Vec<u8>> {
        let params_value = bytes_to_value(&params);
        self.call_function_with_value(function_name, params_value)
            .await
    }

    /// Call an export function with structured Value params.
    ///
    /// Actor state lives *inside* the module (see `docs/in-module-state.md`): the
    /// runtime never threads it through the call, so only the function's own
    /// parameters cross the boundary and only its own return comes back.
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

        // Arm the epoch deadline before entering the guest: with the 1/sec ticker
        // above, a runaway call traps once `ticks` seconds pass and returns Err
        // (an epoch trap) instead of pegging a core. Tight on actor.init (the
        // spine-wedging path), generous otherwise. The deadline is per-call, so a
        // legitimately slow call just needs a bigger budget.
        let epoch_ticks = if function_name == "theater:simple/actor.init" {
            INIT_EPOCH_DEADLINE_TICKS
        } else {
            DEFAULT_EPOCH_DEADLINE_TICKS
        };
        self.instance.set_deadline(epoch_ticks);

        // Diagnostic: capture the EXACT encoded actor.init input (the
        // Tuple[..params] the guest's composite_abi decoder receives) so a
        // hanging/pathological decode input can be handed to packr verbatim.
        // Off by default; the hex-encode only runs when enabled:
        //   RUST_LOG=theater::init_encode_dump=trace
        if function_name == "theater:simple/actor.init" {
            tracing::trace!(
                target: "theater::init_encode_dump",
                hex = %hex::encode(encode_value(&input).unwrap_or_default()),
                "actor.init encoded input bytes (packr encode of Tuple[..params])"
            );
        }

        let output = packr_core::call_with_value(&mut self.instance, function_name, &input)
            .await
            .with_context(|| {
                format!(
                    "Failed to call function '{}' with input: {}",
                    function_name, input
                )
            })?;

        decode_function_result(output)
    }

    /// Call a simple function that takes and returns a Value directly.
    pub async fn call_value(&mut self, function_name: &str, input: &Value) -> Result<Value> {
        packr_core::call_with_value(&mut self.instance, function_name, input)
            .await
            .with_context(|| format!("Failed to call function '{}'", function_name))
    }
}

// =============================================================================
// Handler interface declaration + hashing
// =============================================================================

/// A single function signature within an interface.
#[derive(Debug, Clone)]
pub struct FuncSignature {
    pub name: String,
    pub params: Vec<Type>,
    pub results: Vec<Type>,
}

/// A declared host interface, used for content-addressed compatibility checking
/// between an actor's imported interfaces and the handlers that provide them.
///
/// Hashes are computed with the packr-core interface-hash primitives
/// (`hash_type_in` → `hash_function` → `hash_interface`), the SAME algorithm the
/// actor read side ([`compute_interface_hash`]) uses — so a handler's hash and
/// the actor's embedded import hash are directly comparable.
#[derive(Debug, Clone)]
pub struct InterfaceImpl {
    /// The interface name (e.g., "theater:simple/runtime").
    pub name: String,
    /// Type definitions in scope for ref resolution when hashing.
    pub types: Vec<TypeDef>,
    /// Function signatures declared by this interface.
    pub functions: Vec<FuncSignature>,
}

impl InterfaceImpl {
    /// Build an interface declaration from a parsed `.pact` interface.
    ///
    /// The full interface name is `@package/interface-name`. Interface-level and
    /// type-export typedefs are captured so refs in function signatures resolve
    /// structurally when hashing.
    pub fn from_pact(pact: &PactInterface) -> Self {
        // Get package from metadata (e.g., "theater:simple").
        let package = pact
            .metadata
            .iter()
            .find(|m| m.name == "package")
            .and_then(|m| match &m.value {
                MetadataValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("");

        let full_name = if package.is_empty() {
            pact.name.clone()
        } else {
            format!("{}/{}", package, pact.name)
        };

        // Interface-level typedefs + type-style exports form the ref-resolution
        // scope for function-signature hashing.
        let mut types = pact.types.clone();
        let mut functions = Vec::new();
        for export in &pact.exports {
            match export {
                PactExport::Type(td) => types.push(td.clone()),
                PactExport::Function(func) => {
                    functions.push(FuncSignature {
                        name: func.name.clone(),
                        params: func.params.iter().map(|p| p.ty.clone()).collect(),
                        results: func.results.clone(),
                    });
                }
            }
        }

        Self {
            name: full_name,
            types,
            functions,
        }
    }

    /// The interface name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The function signatures.
    pub fn signatures(&self) -> &[FuncSignature] {
        &self.functions
    }

    fn func_hash(&self, func: &FuncSignature) -> TypeHash {
        let param_hashes: Vec<_> = func
            .params
            .iter()
            .map(|t| packr_core::metadata::hash_type_in(t, &self.types))
            .collect();
        let result_hashes: Vec<_> = func
            .results
            .iter()
            .map(|t| packr_core::metadata::hash_type_in(t, &self.types))
            .collect();
        packr_abi::hash_function(&param_hashes, &result_hashes)
    }

    /// Compute the interface hash over all functions.
    pub fn hash(&self) -> TypeHash {
        let mut bindings: Vec<packr_abi::Binding<'_>> = self
            .functions
            .iter()
            .map(|f| packr_abi::Binding {
                name: &f.name,
                hash: self.func_hash(f),
            })
            .collect();
        bindings.sort_by(|a, b| a.name.cmp(b.name));
        packr_abi::hash_interface(&self.name, &[], &bindings)
    }

    /// Compute the interface hash for a subset of functions.
    ///
    /// Enables partial interface matching — an actor that imports only some
    /// functions from an interface can still verify against a handler that
    /// provides the full interface. Returns `None` if any requested function is
    /// missing.
    pub fn hash_subset(&self, function_names: &[&str]) -> Option<TypeHash> {
        let mut selected = Vec::with_capacity(function_names.len());
        for name in function_names {
            let func = self.functions.iter().find(|f| f.name == *name)?;
            selected.push((func.name.clone(), self.func_hash(func)));
        }
        let mut bindings: Vec<packr_abi::Binding<'_>> = selected
            .iter()
            .map(|(name, hash)| packr_abi::Binding { name, hash: *hash })
            .collect();
        bindings.sort_by(|a, b| a.name.cmp(b.name));
        Some(packr_abi::hash_interface(&self.name, &[], &bindings))
    }

    /// The hash for a single named function, if present.
    pub fn function_hash(&self, name: &str) -> Option<TypeHash> {
        self.functions
            .iter()
            .find(|f| f.name == name)
            .map(|f| self.func_hash(f))
    }
}

// =============================================================================
// Value Conversion Utilities
// =============================================================================

/// Convert bytes to a Value (as a list of u8).
fn bytes_to_value(bytes: &[u8]) -> Value {
    Value::List {
        elem_type: ValueType::U8,
        items: bytes.iter().copied().map(Value::U8).collect(),
    }
}

/// Encode a Value to bytes using the Graph ABI.
pub fn encode_value(value: &Value) -> Result<Vec<u8>> {
    packr_core::abi::encode(value).map_err(|e| anyhow::anyhow!("Failed to encode value: {:?}", e))
}

/// Decode bytes to a Value using the Graph ABI.
pub fn decode_value(bytes: &[u8]) -> Result<Value> {
    packr_core::abi::decode(bytes).map_err(|e| anyhow::anyhow!("Failed to decode value: {:?}", e))
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

// Type→Value conversion is packr's job now: primitives, `Option<T>`, `Vec<T>`,
// etc. impl `From<T> for Value` in packr-abi, and domain types derive it with
// `#[derive(GraphValue)]`. Use `Value::from(x)` / `x.into()` rather than a
// theater-local trait.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn module_cache_hits_on_identical_bytes() {
        let rt = CachingPackRuntime::new();
        // wasmtime's default `wat` feature accepts text modules.
        let wat_a = b"(module)";
        let wat_b = b"(module (func))";

        let (_, hit) = rt.compile_cached(wat_a).await.unwrap();
        assert!(!hit, "first load of A must be a miss");
        assert_eq!(rt.cached_module_count(), 1);

        let (_, hit) = rt.compile_cached(wat_a).await.unwrap();
        assert!(hit, "second load of A must be a hit");
        assert_eq!(rt.cached_module_count(), 1);

        let (_, hit) = rt.compile_cached(wat_b).await.unwrap();
        assert!(!hit, "different bytes must be a miss");
        assert_eq!(rt.cached_module_count(), 2);
    }
}
