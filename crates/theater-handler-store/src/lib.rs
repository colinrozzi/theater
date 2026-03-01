//! # Store Handler
//!
//! Provides content storage capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to store, retrieve, and manage content with labels in a
//! content-addressed storage system.
//!
//! ## Features
//!
//! - Content-addressed storage with SHA1 hashing
//! - Label-based content management
//! - Store, retrieve, list, and delete operations
//! - Permission-based access control
//! - Complete event chain recording for auditability
//!
//! ## Usage
//!
//! ```rust
//! use theater_handler_store::StoreHandler;
//! use theater::config::actor_manifest::StoreHandlerConfig;
//!
//! let config = StoreHandlerConfig::default();
//! let handler = StoreHandler::new(config, None);
//! ```

pub mod events;


use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tracing::{debug, error, info};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::actor_manifest::StoreHandlerConfig;
use theater::config::permissions::StorePermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::store::{ContentRef, ContentStore, Label};

// Pack integration
use theater::pack_bridge::{
    AsyncCtx, Ctx, HostLinkerBuilder, InterfaceImpl, LinkerError, TypeHash, Value,
    parse_pact,
};

// ============================================================================
// Interface Declarations
// ============================================================================

/// Embedded store.pact file content
const STORE_PACT: &str = include_str!("../../../pact/store.pact");

/// Declare the theater:simple/store interface from the pact file.
/// Content-refs are represented as strings (hash values) for simplicity.
///
/// Functions for content-addressed storage:
/// - new() -> result<string, string>                                           (returns store-id)
/// - store(store-id, content: list<u8>) -> result<string, string>              (returns content-ref hash)
/// - get(store-id, content-ref: string) -> result<list<u8>, string>            (content-ref is hash string)
/// - exists(store-id, content-ref: string) -> result<bool, string>
/// - label(store-id, label, content-ref: string) -> result<_, string>
/// - get-by-label(store-id, label) -> result<option<string>, string>           (returns option<content-ref hash>)
/// - remove-label(store-id, label) -> result<_, string>
/// - store-at-label(store-id, label, content: list<u8>) -> result<string, string>
/// - replace-content-at-label(store-id, label, content: list<u8>) -> result<string, string>
/// - replace-at-label(store-id, label, content-ref: string) -> result<_, string>
/// - list-all-content(store-id) -> result<list<string>, string>                (list of content-ref hashes)
/// - calculate-total-size(store-id) -> result<u64, string>
/// - list-labels(store-id) -> result<list<string>, string>
fn store_interface() -> InterfaceImpl {
    let pact = parse_pact(STORE_PACT)
        .expect("embedded store.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// Errors that can occur during store operations
#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Handler for providing content storage access to WebAssembly actors
#[derive(Clone)]
pub struct StoreHandler {
    #[allow(dead_code)]
    permissions: Option<StorePermissions>,
    /// Custom base path for content storage. If None, uses the default theater home location.
    base_path: Option<std::path::PathBuf>,
}

impl StoreHandler {
    /// Create a new store handler with the given configuration and permissions
    pub fn new(
        config: StoreHandlerConfig,
        permissions: Option<StorePermissions>,
    ) -> Self {
        Self {
            permissions,
            base_path: config.base_path,
        }
    }

    /// Get the interface declarations for this handler.
    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![store_interface()]
    }
}

// Helper functions for parsing Composite Value inputs

fn parse_content_ref(value: &Value) -> Result<ContentRef, Value> {
    match value {
        Value::String(hash) => Ok(ContentRef::new(hash.clone())),
        _ => Err(Value::String("Expected string for content-ref".to_string())),
    }
}

fn parse_store_id(input: &Value) -> Result<String, Value> {
    match input {
        Value::String(s) => Ok(s.clone()),
        Value::Tuple(fields) if fields.len() == 1 => {
            match &fields[0] {
                Value::String(s) => Ok(s.clone()),
                _ => Err(Value::String("Expected string for store_id".to_string())),
            }
        }
        _ => Err(Value::String("Expected string for store_id".to_string())),
    }
}

fn parse_store_id_and_ref(input: &Value) -> Result<(String, ContentRef), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let store_id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for store_id".to_string())),
            };
            let content_ref = parse_content_ref(&fields[1])?;
            Ok((store_id, content_ref))
        }
        _ => Err(Value::String("Expected tuple (store_id, content_ref)".to_string())),
    }
}

fn parse_store_id_and_label(input: &Value) -> Result<(String, String), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 2 => {
            let store_id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for store_id".to_string())),
            };
            let label = match &fields[1] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for label".to_string())),
            };
            Ok((store_id, label))
        }
        _ => Err(Value::String("Expected tuple (store_id, label)".to_string())),
    }
}

fn parse_store_label_ref(input: &Value) -> Result<(String, String, ContentRef), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 3 => {
            let store_id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for store_id".to_string())),
            };
            let label = match &fields[1] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for label".to_string())),
            };
            let content_ref = parse_content_ref(&fields[2])?;
            Ok((store_id, label, content_ref))
        }
        _ => Err(Value::String("Expected tuple (store_id, label, content_ref)".to_string())),
    }
}

fn parse_store_label_content(input: &Value) -> Result<(String, String, Vec<u8>), Value> {
    match input {
        Value::Tuple(fields) if fields.len() == 3 => {
            let store_id = match &fields[0] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for store_id".to_string())),
            };
            let label = match &fields[1] {
                Value::String(s) => s.clone(),
                _ => return Err(Value::String("Expected string for label".to_string())),
            };
            let content = match &fields[2] {
                Value::List { items, .. } => {
                    items.iter().filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    }).collect::<Vec<u8>>()
                }
                _ => return Err(Value::String("Expected list<u8> for content".to_string())),
            };
            Ok((store_id, label, content))
        }
        _ => Err(Value::String("Expected tuple (store_id, label, content)".to_string())),
    }
}

impl Handler for StoreHandler
{
    fn create_instance(&self, config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        use theater::config::actor_manifest::HandlerConfig;

        if let Some(HandlerConfig::Store { config: store_config }) = config {
            Box::new(StoreHandler::new(store_config.clone(), self.permissions.clone()))
        } else {
            Box::new(self.clone())
        }
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: tokio::sync::broadcast::Receiver<theater::chain::ChainEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Store handler setup");

        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("Store handler received shutdown signal");
            Ok(())
        })
    }

    fn name(&self) -> &str {
        "store"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(self.interfaces().iter().map(|i| i.name().to_string()).collect())
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<theater::pack_bridge::InterfaceImpl> {
        vec![store_interface()]
    }

    // =========================================================================
    // Composite Integration
    // =========================================================================

    fn setup_host_functions_composite(
        &mut self,
        builder: &mut HostLinkerBuilder<'_, ActorStore>,
        ctx: &mut HandlerContext,
    ) -> Result<(), LinkerError> {
        info!("Setting up store host functions (Pack)");

        // Check if already satisfied
        if ctx.is_satisfied("theater:simple/store") {
            info!("theater:simple/store already satisfied, skipping");
            return Ok(());
        }

        // Clone base_path for use in each closure
        let bp_new = self.base_path.clone();
        let bp_store = self.base_path.clone();
        let bp_get = self.base_path.clone();
        let bp_exists = self.base_path.clone();
        let bp_label = self.base_path.clone();
        let bp_get_by_label = self.base_path.clone();
        let bp_remove_label = self.base_path.clone();
        let bp_store_at_label = self.base_path.clone();
        let bp_replace_content = self.base_path.clone();
        let bp_replace_at = self.base_path.clone();
        let bp_list_all = self.base_path.clone();
        let bp_calc_size = self.base_path.clone();
        let bp_list_labels = self.base_path.clone();

        // Helper to create store from id with optional base path
        fn make_store(store_id: &str, base_path: &Option<std::path::PathBuf>) -> ContentStore {
            if let Some(ref bp) = base_path {
                ContentStore::from_id_with_base_path(store_id, bp.clone())
            } else {
                ContentStore::from_id(store_id)
            }
        }

        builder
            .interface("theater:simple/store")?
            // new() -> result<string, string>
            .func_typed("new", move |_ctx: &mut Ctx<'_, ActorStore>, _input: Value| {
                let store = if let Some(ref bp) = bp_new {
                    ContentStore::new_with_base_path(bp.clone())
                } else {
                    ContentStore::new()
                };
                // Return Ok(store_id) as Variant with tag 0
                Value::Variant {
                    type_name: String::from("result"),
                    case_name: String::from("ok"),
                    tag: 0, // ok
                    payload: vec![Value::String(store.id().to_string())],
                }
            })?
            // store(store-id: string, content: list<u8>) -> result<string, string>
            .func_async_result("store", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_store.clone();
                async move {
                    // Parse input tuple: (store_id, content)
                    let (store_id, content) = match input {
                        Value::Tuple(fields) if fields.len() == 2 => {
                            let store_id = match &fields[0] {
                                Value::String(s) => s.clone(),
                                _ => return Err(Value::String("Expected string for store_id".to_string())),
                            };
                            let content = match &fields[1] {
                                Value::List { items, .. } => {
                                    items.iter().filter_map(|v| match v {
                                        Value::U8(b) => Some(*b),
                                        _ => None,
                                    }).collect::<Vec<u8>>()
                                }
                                _ => return Err(Value::String("Expected list<u8> for content".to_string())),
                            };
                            (store_id, content)
                        }
                        _ => return Err(Value::String("Expected tuple (store_id, content)".to_string())),
                    };

                    let store = make_store(&store_id, &bp);
                    let content_ref = store.store(content).await;
                    debug!("Content stored successfully: {}", content_ref.hash());

                    // Return content-ref as string (the hash)
                    Ok(Value::String(content_ref.hash().to_string()))
                }
            })?
            // get(store-id: string, content-ref: content-ref) -> result<list<u8>, string>
            .func_async_result("get", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_get.clone();
                async move {
                    let (store_id, content_ref) = parse_store_id_and_ref(&input)?;
                    let store = make_store(&store_id, &bp);

                    match store.get(&content_ref).await {
                        Ok(content) => {
                            use theater::ValueType;
                            debug!("Content retrieved successfully");
                            Ok(Value::List {
                                elem_type: ValueType::U8,
                                items: content.into_iter().map(Value::U8).collect(),
                            })
                        }
                        Err(e) => {
                            error!("Error retrieving content: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // exists(store-id: string, content-ref: content-ref) -> result<bool, string>
            .func_async_result("exists", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_exists.clone();
                async move {
                    let (store_id, content_ref) = parse_store_id_and_ref(&input)?;
                    let store = make_store(&store_id, &bp);

                    let exists = store.exists(&content_ref).await;
                    debug!("Content existence checked successfully");
                    Ok::<Value, Value>(Value::Bool(exists))
                }
            })?
            // label(store-id: string, label: string, content-ref: content-ref) -> result<_, string>
            .func_async_result("label", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_label.clone();
                async move {
                    let (store_id, label_string, content_ref) = parse_store_label_ref(&input)?;
                    let store = make_store(&store_id, &bp);
                    let label = Label::new(label_string);

                    match store.label(&label, &content_ref).await {
                        Ok(_) => {
                            debug!("Content labeled successfully");
                            Ok(Value::Tuple(vec![]))
                        }
                        Err(e) => {
                            error!("Error labeling content: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // get-by-label(store-id: string, label: string) -> result<option<string>, string>
            .func_async_result("get-by-label", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_get_by_label.clone();
                async move {
                    let (store_id, label_string) = parse_store_id_and_label(&input)?;
                    let store = make_store(&store_id, &bp);
                    let label = Label::new(label_string);

                    match store.get_by_label(&label).await {
                        Ok(content_ref_opt) => {
                            use theater::ValueType;
                            debug!("Content reference by label retrieved successfully");
                            match content_ref_opt {
                                Some(cr) => Ok(Value::Option {
                                    inner_type: ValueType::String,
                                    value: Some(Box::new(Value::String(cr.hash().to_string()))),
                                }),
                                None => Ok(Value::Option {
                                    inner_type: ValueType::String,
                                    value: None,
                                }),
                            }
                        }
                        Err(e) => {
                            error!("Error retrieving content by label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // remove-label(store-id: string, label: string) -> result<_, string>
            .func_async_result("remove-label", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_remove_label.clone();
                async move {
                    let (store_id, label_string) = parse_store_id_and_label(&input)?;
                    let store = make_store(&store_id, &bp);
                    let label = Label::new(label_string);

                    match store.remove_label(&label).await {
                        Ok(_) => {
                            debug!("Label removed successfully");
                            Ok(Value::Tuple(vec![]))
                        }
                        Err(e) => {
                            error!("Error removing label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // store-at-label(store-id: string, label: string, content: list<u8>) -> result<string, string>
            .func_async_result("store-at-label", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_store_at_label.clone();
                async move {
                    let (store_id, label_string, content) = parse_store_label_content(&input)?;
                    let store = make_store(&store_id, &bp);
                    let label = Label::new(label_string);

                    match store.store_at_label(&label, content).await {
                        Ok(content_ref) => {
                            debug!("Content stored at label successfully");
                            Ok(Value::String(content_ref.hash().to_string()))
                        }
                        Err(e) => {
                            error!("Error storing content at label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // replace-content-at-label(store-id: string, label: string, content: list<u8>) -> result<string, string>
            .func_async_result("replace-content-at-label", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_replace_content.clone();
                async move {
                    let (store_id, label_string, content) = parse_store_label_content(&input)?;
                    let store = make_store(&store_id, &bp);
                    let label = Label::new(label_string);

                    match store.replace_content_at_label(&label, content).await {
                        Ok(content_ref) => {
                            debug!("Content at label replaced successfully");
                            Ok(Value::String(content_ref.hash().to_string()))
                        }
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // replace-at-label(store-id: string, label: string, content-ref: content-ref) -> result<_, string>
            .func_async_result("replace-at-label", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_replace_at.clone();
                async move {
                    let (store_id, label_string, content_ref) = parse_store_label_ref(&input)?;
                    let store = make_store(&store_id, &bp);
                    let label = Label::new(label_string);

                    match store.replace_at_label(&label, &content_ref).await {
                        Ok(_) => {
                            debug!("Content at label replaced with reference successfully");
                            Ok(Value::Tuple(vec![]))
                        }
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // list-all-content(store-id: string) -> result<list<string>, string>
            .func_async_result("list-all-content", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_list_all.clone();
                async move {
                    let store_id = parse_store_id(&input)?;
                    let store = make_store(&store_id, &bp);

                    match store.list_all_content().await {
                        Ok(content_refs) => {
                            use theater::ValueType;
                            debug!("All content references listed successfully");
                            let refs: Vec<Value> = content_refs
                                .into_iter()
                                .map(|cr| Value::String(cr.hash().to_string()))
                                .collect();
                            Ok(Value::List {
                                elem_type: ValueType::String,
                                items: refs,
                            })
                        }
                        Err(e) => {
                            error!("Error listing all content: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // calculate-total-size(store-id: string) -> result<u64, string>
            .func_async_result("calculate-total-size", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_calc_size.clone();
                async move {
                    let store_id = parse_store_id(&input)?;
                    let store = make_store(&store_id, &bp);

                    match store.calculate_total_size().await {
                        Ok(total_size) => {
                            debug!("Total size calculated successfully");
                            Ok(Value::U64(total_size))
                        }
                        Err(e) => {
                            error!("Error calculating total size: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // list-labels(store-id: string) -> result<list<string>, string>
            .func_async_result("list-labels", move |_ctx: AsyncCtx<ActorStore>, input: Value| {
                let bp = bp_list_labels.clone();
                async move {
                    let store_id = parse_store_id(&input)?;
                    let store = make_store(&store_id, &bp);

                    match store.list_labels().await {
                        Ok(labels) => {
                            use theater::ValueType;
                            debug!("Labels listed successfully");
                            let label_values: Vec<Value> = labels
                                .into_iter()
                                .map(Value::String)
                                .collect();
                            Ok(Value::List {
                                elem_type: ValueType::String,
                                items: label_values,
                            })
                        }
                        Err(e) => {
                            error!("Error listing labels: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?;

        ctx.mark_satisfied("theater:simple/store");
        info!("Store host functions (Pack) set up successfully");
        Ok(())
    }

    fn supports_composite(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_handler_creation() {
        let config = StoreHandlerConfig::default();
        let handler = StoreHandler::new(config, None);

        assert_eq!(handler.name(), "store");
        assert_eq!(handler.imports(), Some(vec!["theater:simple/store".to_string()]));
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_store_handler_clone() {
        let config = StoreHandlerConfig::default();
        let handler = StoreHandler::new(config, None);
        let cloned = handler.create_instance(None);

        assert_eq!(cloned.name(), "store");
    }

    #[test]
    fn test_store_interface_hash_determinism() {
        let interface1 = store_interface();
        let interface2 = store_interface();
        assert_eq!(interface1.hash(), interface2.hash());
    }

    #[test]
    fn test_store_handler_interface_hashes() {
        let config = StoreHandlerConfig::default();
        let handler = StoreHandler::new(config, None);

        let hashes = handler.interface_hashes();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "theater:simple/store");

        // Hash should be non-zero
        assert!(!hashes[0].1.as_bytes().iter().all(|&b| b == 0));
    }
}
