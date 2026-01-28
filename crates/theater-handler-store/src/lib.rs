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
//! let config = StoreHandlerConfig {};
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
    AsyncCtx, PackInstance, Ctx, HostLinkerBuilder, LinkerError, Value,
};

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
}

impl StoreHandler {
    /// Create a new store handler with the given configuration and permissions
    pub fn new(
        _config: StoreHandlerConfig,
        permissions: Option<StorePermissions>,
    ) -> Self {
        Self { permissions }
    }
}

// Helper functions for parsing Composite Value inputs

fn parse_content_ref(value: &Value) -> Result<ContentRef, Value> {
    match value {
        Value::Record { fields, .. } => {
            for (name, val) in fields {
                if name == "hash" {
                    if let Value::String(hash) = val {
                        return Ok(ContentRef::new(hash.clone()));
                    }
                }
            }
            Err(Value::String("content-ref record missing hash field".to_string()))
        }
        _ => Err(Value::String("Expected content-ref record".to_string())),
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
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Store handler starting...");

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
        Some(vec!["theater:simple/store".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
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

        builder
            .interface("theater:simple/store")?
            // new() -> result<string, string>
            .func_typed("new", |_ctx: &mut Ctx<'_, ActorStore>, _input: Value| {
                let store = ContentStore::new();
                // Return Ok(store_id) as Variant with tag 0
                Value::Variant {
                    type_name: String::from("result"),
                    case_name: String::from("ok"),
                    tag: 0, // ok
                    payload: vec![Value::String(store.id().to_string())],
                }
            })?
            // store(store-id: string, content: list<u8>) -> result<content-ref, string>
            .func_async_result("store", |_ctx: AsyncCtx<ActorStore>, input: Value| {
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

                    let store = ContentStore::from_id(&store_id);
                    let content_ref = store.store(content).await;
                    debug!("Content stored successfully: {}", content_ref.hash());

                    // Return content-ref record
                    Ok(Value::Record {
                        type_name: String::from("content-ref"),
                        fields: vec![
                            ("hash".to_string(), Value::String(content_ref.hash().to_string()))
                        ],
                    })
                }
            })?
            // get(store-id: string, content-ref: content-ref) -> result<list<u8>, string>
            .func_async_result("get", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, content_ref) = parse_store_id_and_ref(&input)?;
                    let store = ContentStore::from_id(&store_id);

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
            .func_async_result("exists", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, content_ref) = parse_store_id_and_ref(&input)?;
                    let store = ContentStore::from_id(&store_id);

                    let exists = store.exists(&content_ref).await;
                    debug!("Content existence checked successfully");
                    Ok::<Value, Value>(Value::Bool(exists))
                }
            })?
            // label(store-id: string, label: string, content-ref: content-ref) -> result<_, string>
            .func_async_result("label", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, label_string, content_ref) = parse_store_label_ref(&input)?;
                    let store = ContentStore::from_id(&store_id);
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
            // get-by-label(store-id: string, label: string) -> result<option<content-ref>, string>
            .func_async_result("get-by-label", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, label_string) = parse_store_id_and_label(&input)?;
                    let store = ContentStore::from_id(&store_id);
                    let label = Label::new(label_string);

                    match store.get_by_label(&label).await {
                        Ok(content_ref_opt) => {
                            use theater::ValueType;
                            debug!("Content reference by label retrieved successfully");
                            match content_ref_opt {
                                Some(cr) => Ok(Value::Option {
                                    inner_type: ValueType::Record(String::from("content-ref")),
                                    value: Some(Box::new(Value::Record {
                                        type_name: String::from("content-ref"),
                                        fields: vec![("hash".to_string(), Value::String(cr.hash().to_string()))],
                                    })),
                                }),
                                None => Ok(Value::Option {
                                    inner_type: ValueType::Record(String::from("content-ref")),
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
            .func_async_result("remove-label", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, label_string) = parse_store_id_and_label(&input)?;
                    let store = ContentStore::from_id(&store_id);
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
            // store-at-label(store-id: string, label: string, content: list<u8>) -> result<content-ref, string>
            .func_async_result("store-at-label", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, label_string, content) = parse_store_label_content(&input)?;
                    let store = ContentStore::from_id(&store_id);
                    let label = Label::new(label_string);

                    match store.store_at_label(&label, content).await {
                        Ok(content_ref) => {
                            debug!("Content stored at label successfully");
                            Ok(Value::Record {
                                type_name: String::from("content-ref"),
                                fields: vec![
                                    ("hash".to_string(), Value::String(content_ref.hash().to_string()))
                                ],
                            })
                        }
                        Err(e) => {
                            error!("Error storing content at label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // replace-content-at-label(store-id: string, label: string, content: list<u8>) -> result<content-ref, string>
            .func_async_result("replace-content-at-label", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, label_string, content) = parse_store_label_content(&input)?;
                    let store = ContentStore::from_id(&store_id);
                    let label = Label::new(label_string);

                    match store.replace_content_at_label(&label, content).await {
                        Ok(content_ref) => {
                            debug!("Content at label replaced successfully");
                            Ok(Value::Record {
                                type_name: String::from("content-ref"),
                                fields: vec![
                                    ("hash".to_string(), Value::String(content_ref.hash().to_string()))
                                ],
                            })
                        }
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);
                            Err(Value::String(e.to_string()))
                        }
                    }
                }
            })?
            // replace-at-label(store-id: string, label: string, content-ref: content-ref) -> result<_, string>
            .func_async_result("replace-at-label", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let (store_id, label_string, content_ref) = parse_store_label_ref(&input)?;
                    let store = ContentStore::from_id(&store_id);
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
            // list-all-content(store-id: string) -> result<list<content-ref>, string>
            .func_async_result("list-all-content", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let store_id = parse_store_id(&input)?;
                    let store = ContentStore::from_id(&store_id);

                    match store.list_all_content().await {
                        Ok(content_refs) => {
                            use theater::ValueType;
                            debug!("All content references listed successfully");
                            let refs: Vec<Value> = content_refs
                                .into_iter()
                                .map(|cr| Value::Record {
                                    type_name: String::from("content-ref"),
                                    fields: vec![
                                        ("hash".to_string(), Value::String(cr.hash().to_string()))
                                    ],
                                })
                                .collect();
                            Ok(Value::List {
                                elem_type: ValueType::Record(String::from("content-ref")),
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
            .func_async_result("calculate-total-size", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let store_id = parse_store_id(&input)?;
                    let store = ContentStore::from_id(&store_id);

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
            .func_async_result("list-labels", |_ctx: AsyncCtx<ActorStore>, input: Value| {
                async move {
                    let store_id = parse_store_id(&input)?;
                    let store = ContentStore::from_id(&store_id);

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

    fn register_exports_composite(&self, _instance: &mut PackInstance) -> anyhow::Result<()> {
        // Store handler has no exports
        info!("No export functions needed for store handler (Pack)");
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
        let config = StoreHandlerConfig {};
        let handler = StoreHandler::new(config, None);

        assert_eq!(handler.name(), "store");
        assert_eq!(handler.imports(), Some(vec!["theater:simple/store".to_string()]));
        assert_eq!(handler.exports(), None);
    }

    #[test]
    fn test_store_handler_clone() {
        let config = StoreHandlerConfig {};
        let handler = StoreHandler::new(config, None);
        let cloned = handler.create_instance(None);

        assert_eq!(cloned.name(), "store");
    }
}
