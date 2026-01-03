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

pub use events::StoreEventData;

use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::actor_manifest::StoreHandlerConfig;
use theater::config::permissions::StorePermissions;
use theater::events::{ChainEventData, EventPayload};
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::store::{ContentRef, ContentStore, Label};
use theater::wasm::{ActorComponent, ActorInstance};

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

impl<E> Handler<E> for StoreHandler
where
    E: EventPayload + Clone + From<StoreEventData>,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Store handler starting...");

        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("Store handler received shutdown signal");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
        _ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "store-setup".to_string(),
            data: StoreEventData::HandlerSetupStart.into(),
            description: Some("Starting store host function setup".to_string()),
        });

        info!("Setting up store host functions");

        let mut interface = match actor_component.linker.instance("theater:simple/store") {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "store-setup".to_string(),
                    data: StoreEventData::LinkerInstanceSuccess.into(),
                            description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "store-setup".to_string(),
                    data: StoreEventData::HandlerSetupError {                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }.into(),
                            description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/store: {}",
                    e
                ));
            }
        };

        // Setup: new() - Create a new content store
        interface.func_wrap(
            "new",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (): ()| -> Result<(Result<String, String>,), anyhow::Error> {
                // Record store call event
                ctx.data_mut().record_handler_event("theater:simple/store/new".to_string(), StoreEventData::NewStoreCall {}, Some("Creating new content store".to_string()));

                let store = ContentStore::new();

                // Record store result event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/new".to_string(),
                    data: StoreEventData::NewStoreResult {                        store_id: store.id().to_string(),
                        success: true,
                    }.into(),
                            description: Some(format!("New content store created with ID: {}", store.id())),
                });

                Ok((Ok(store.id().to_string()),))
            },
        )?;

        // Setup: store() - Store content
        interface.func_wrap_async(
            "store",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (store_id, content): (String, Vec<u8>)|
                -> Box<dyn Future<Output = Result<(Result<ContentRef, String>,), anyhow::Error>> + Send> {
                // Record store call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/store".to_string(),
                    data: StoreEventData::StoreCall {                        store_id: store_id.clone(),
                        content: content.clone(),
                    }.into(),
                            description: Some(format!("Storing {} bytes of content", content.len())),
                });

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the actual store operation (async)
                    let content_ref = store.store(content).await;
                    debug!("Content stored successfully: {}", content_ref.hash());

                    // Record store result event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/store/store".to_string(),
                        data: StoreEventData::StoreResult {                            store_id: store_id.clone(),
                            content_ref: content_ref.clone(),
                            success: true,
                        }.into(),
                                    description: Some(format!("Content stored successfully with hash: {}", content_ref.hash())),
                    });

                    Ok((Ok(ContentRef::from(content_ref)),))
                })
            },
        )?;

        // Setup: get() - Retrieve content
        interface.func_wrap_async(
            "get",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (store_id, content_ref): (String, ContentRef)|
                -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,), anyhow::Error>> + Send> {
                // Record get call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/get".to_string(),
                    data: StoreEventData::GetCall {                        store_id: store_id.clone(),
                        content_ref: content_ref.clone(),
                    }.into(),
                            description: Some(format!("Getting content with hash: {}", content_ref.hash())),
                });

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.get(&content_ref).await {
                        Ok(content) => {
                            debug!("Content retrieved successfully");

                            // Record get result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/get".to_string(),
                                data: StoreEventData::GetResult {                                    store_id: store_id.clone(),
                                    content_ref: content_ref.clone(),
                                    content: Some(content.clone()),
                                    success: true,
                                }.into(),
                                                    description: Some(format!("Retrieved {} bytes of content with hash: {}", content.len(), content_ref.hash())),
                            });

                            Ok((Ok(content),))
                        },
                        Err(e) => {
                            error!("Error retrieving content: {}", e);

                            // Record get error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/get".to_string(),
                                data: StoreEventData::Error {                                    operation: "get".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!("Error retrieving content with hash {}: {}", content_ref.hash(), e)),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: exists() - Check if content exists
        interface.func_wrap_async(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (store_id, content_ref): (String, ContentRef)|
                  -> Box<dyn Future<Output = Result<(Result<bool, String>,), anyhow::Error>> + Send> {
                // Record exists call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/exists".to_string(),
                    data: StoreEventData::ExistsCall {                        store_id: store_id.clone(),
                        content_ref: content_ref.clone(),
                    }.into(),
                            description: Some(format!(
                        "Checking if content with hash {} exists",
                        content_ref.hash()
                    )),
                });

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    let exists = store.exists(&content_ref).await;
                    debug!("Content existence checked successfully");

                    // Record exists result event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/store/exists".to_string(),
                        data: StoreEventData::ExistsResult {                            store_id: store_id.clone(),
                            content_ref: content_ref.clone(),
                            exists,
                            success: true,
                        }.into(),
                                    description: Some(format!(
                            "Content with hash {} exists: {}",
                            content_ref.hash(),
                            exists
                        )),
                    });

                    Ok((Ok(exists),))
                })
            },
        )?;

        // Setup: label() - Add a label to content
        interface.func_wrap_async(
            "label",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (store_id, label_string, content_ref): (String, String, ContentRef)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,), anyhow::Error>> + Send> {
                // Record label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/label".to_string(),
                    data: StoreEventData::LabelCall {                        store_id: store_id.clone(),
                        label: label_string.clone(),
                        content_ref: content_ref.clone(),
                    }.into(),
                            description: Some(format!(
                        "Labeling content with hash {} as '{}'",
                        content_ref.hash(),
                        label_string
                    )),
                });

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.label(&label, &content_ref).await {
                        Ok(_) => {
                            debug!("Content labeled successfully");

                            // Record label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/label".to_string(),
                                data: StoreEventData::LabelResult {                                    store_id: store_id.clone(),
                                    label: label_string.clone(),
                                    content_ref: content_ref.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!(
                                    "Successfully labeled content with hash {} as '{}'",
                                    content_ref.hash(),
                                    label_clone
                                )),
                            });

                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            error!("Error labeling content: {}", e);

                            // Record label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/label".to_string(),
                                data: StoreEventData::Error {                                    operation: "label".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!(
                                    "Error labeling content with hash {} as '{}': {}",
                                    content_ref.hash(),
                                    label_clone,
                                    e
                                )),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: get-by-label() - Get content reference by label
        interface.func_wrap_async(
            "get-by-label",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (store_id, label_string): (String, String)|
                  -> Box<dyn Future<Output = Result<(Result<Option<ContentRef>, String>,), anyhow::Error>> + Send> {
                // Record get-by-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/get-by-label".to_string(),
                    data: StoreEventData::GetByLabelCall {                        store_id: store_id.clone(),
                        label: label_string.clone(),
                    }.into(),
                            description: Some(format!(
                        "Getting content reference by label: {}",
                        label_string
                    )),
                });

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.get_by_label(&label).await {
                        Ok(content_ref_opt) => {
                            debug!("Content reference by label retrieved successfully");

                            // Record get-by-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/get-by-label".to_string(),
                                data: StoreEventData::GetByLabelResult {                                    store_id: store_id.clone(),
                                    label: label_clone.name().to_string(),
                                    content_ref: content_ref_opt.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!(
                                    "Successfully retrieved content reference {:?} for label '{}'",
                                    content_ref_opt, label_clone
                                )),
                            });

                            Ok((Ok(content_ref_opt),))
                        }
                        Err(e) => {
                            error!("Error retrieving content reference by label: {}", e);

                            // Record get-by-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/get-by-label".to_string(),
                                data: StoreEventData::Error {                                    operation: "get-by-label".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!(
                                    "Error retrieving content reference for label '{}': {}",
                                    label_clone, e
                                )),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: remove-label() - Remove a label
        interface.func_wrap_async(
            "remove-label",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (store_id, label_string): (String, String)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,), anyhow::Error>> + Send> {
                // Record remove-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/remove-label".to_string(),
                    data: StoreEventData::RemoveLabelCall {                        store_id: store_id.clone(),
                        label: label_string.clone(),
                    }.into(),
                            description: Some(format!("Removing label: {}", label_string)),
                });

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.remove_label(&label).await {
                        Ok(_) => {
                            debug!("Label removed successfully");

                            // Record remove-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/remove-label".to_string(),
                                data: StoreEventData::RemoveLabelResult {                                    store_id: store_id.clone(),
                                    label: label_string.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!(
                                    "Successfully removed label '{}'",
                                    label_clone
                                )),
                            });

                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            error!("Error removing label: {}", e);

                            // Record remove-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/remove-label".to_string(),
                                data: StoreEventData::Error {                                    operation: "remove-label".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!(
                                    "Error removing label '{}': {}",
                                    label_clone, e
                                )),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: store-at-label() - Store content and label it
        interface.func_wrap_async(
            "store-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (store_id, label_string, content): (String, String, Vec<u8>)|
                -> Box<dyn Future<Output = Result<(Result<ContentRef, String>,), anyhow::Error>> + Send> {
                // Record store-at-label call event
                ctx.data_mut().record_handler_event(
                    "theater:simple/store/store-at-label".to_string(),
                    StoreEventData::StoreAtLabelCall {
                        store_id: store_id.clone(),
                        label: label_string.clone(),
                        content: content.clone(),
                    },
                    Some(format!("Storing {} bytes of content at label: {}", content.len(), label_string)),
                );

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.store_at_label(&label, content).await {
                        Ok(content_ref) => {
                            debug!("Content stored at label successfully");
                            let content_ref_wit = ContentRef::from(content_ref.clone());

                            // Record store-at-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/store-at-label".to_string(),
                                data: StoreEventData::StoreAtLabelResult {                                    store_id: store_id.clone(),
                                    label: label_string.clone(),
                                    content_ref: content_ref.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!("Successfully stored content with hash {} at label '{}'", content_ref.hash(), label_clone)),
                            });

                            Ok((Ok(content_ref_wit),))
                        },
                        Err(e) => {
                            error!("Error storing content at label [{}]: {}", label, e);

                            // Record store-at-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/store-at-label".to_string(),
                                data: StoreEventData::Error {                                    operation: "store-at-label".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!("Error storing content at label '{}': {}", label_clone, e)),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: replace-content-at-label() - Replace content at a label
        interface.func_wrap_async(
            "replace-content-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (store_id, label_string, content): (String, String, Vec<u8>)|
                -> Box<dyn Future<Output = Result<(Result<ContentRef, String>,), anyhow::Error>> + Send> {
                // Record replace-content-at-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/replace-content-at-label".to_string(),
                    data: StoreEventData::ReplaceContentAtLabelCall {                        store_id: store_id.clone(),
                        label: label_string.clone(),
                        content: content.clone(),
                    }.into(),
                            description: Some(format!("Replacing content at label {} with {} bytes of new content", label_string, content.len())),
                });

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.replace_content_at_label(&label, content).await {
                        Ok(content_ref) => {
                            debug!("Content at label replaced successfully");
                            let content_ref_wit = ContentRef::from(content_ref.clone());

                            // Record replace-content-at-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/replace-content-at-label".to_string(),
                                data: StoreEventData::ReplaceContentAtLabelResult {                                    store_id: store_id.clone(),
                                    label: label_string.clone(),
                                    content_ref: content_ref.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!("Successfully replaced content at label '{}' with new content (hash: {})", label_clone, content_ref.hash())),
                            });

                            Ok((Ok(content_ref_wit),))
                        },
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);

                            // Record replace-content-at-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/replace-content-at-label".to_string(),
                                data: StoreEventData::Error {                                    operation: "replace-content-at-label".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!("Error replacing content at label '{}': {}", label_clone, e)),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: replace-at-label() - Replace content reference at a label
        interface.func_wrap_async(
            "replace-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (store_id, label_string, content_ref): (String, String, ContentRef)|
                -> Box<dyn Future<Output = Result<(Result<(), String>,), anyhow::Error>> + Send> {
                // Record replace-at-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "theater:simple/store/replace-at-label".to_string(),
                    data: StoreEventData::ReplaceAtLabelCall {                        store_id: store_id.clone(),
                        label: label_string.clone(),
                        content_ref: content_ref.clone(),
                    }.into(),
                            description: Some(format!("Replacing content at label {} with content reference: {}", label_string, content_ref.hash())),
                });

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.replace_at_label(&label, &content_ref).await {
                        Ok(_) => {
                            debug!("Content at label replaced with reference successfully");

                            // Record replace-at-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/replace-at-label".to_string(),
                                data: StoreEventData::ReplaceAtLabelResult {                                    store_id: store_id.clone(),
                                    label: label_string.clone(),
                                    content_ref: content_ref.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!("Successfully replaced content at label '{}' with content reference (hash: {})", label_clone, content_ref.hash())),
                            });

                            Ok((Ok(()),))
                        },
                        Err(e) => {
                            error!("Error replacing content at label with reference: {}", e);

                            // Record replace-at-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/replace-at-label".to_string(),
                                data: StoreEventData::Error {                                    operation: "replace-at-label".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!("Error replacing content at label '{}' with reference (hash: {}): {}", label_clone, content_ref.hash(), e)),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: list-all-content() - List all content references
        interface.func_wrap_async(
            "list-all-content",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (store_id,): (String,)|
                  -> Box<dyn Future<Output = Result<(Result<Vec<ContentRef>, String>,), anyhow::Error>> + Send> {
                // Record list-all-content call event
                ctx.data_mut().record_handler_event("theater:simple/store/list-all-content".to_string(), StoreEventData::ListAllContentCall {
                        store_id: store_id.clone(),
                    }, Some("Listing all content references".to_string()));

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.list_all_content().await {
                        Ok(content_refs) => {
                            debug!("All content references listed successfully");

                            // Record list-all-content result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/list-all-content".to_string(),
                                data: StoreEventData::ListAllContentResult {                                    store_id: store_id.clone(),
                                    content_refs: content_refs.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!(
                                    "Successfully listed {} content references",
                                    content_refs.len()
                                )),
                            });

                            Ok((Ok(content_refs),))
                        }
                        Err(e) => {
                            error!("Error listing all content references: {}", e);

                            // Record list-all-content error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/list-all-content".to_string(),
                                data: StoreEventData::Error {                                    operation: "list-all-content".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!(
                                    "Error listing all content references: {}",
                                    e
                                )),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: calculate-total-size() - Calculate total size of all content
        interface.func_wrap_async(
            "calculate-total-size",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                  (store_id,): (String,)|
                  -> Box<dyn Future<Output = Result<(Result<u64, String>,), anyhow::Error>> + Send> {
                // Record calculate-total-size call event
                ctx.data_mut().record_handler_event("theater:simple/store/calculate-total-size".to_string(), StoreEventData::CalculateTotalSizeCall {
                        store_id: store_id.clone(),
                    }, Some("Calculating total size of all content".to_string()));

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.calculate_total_size().await {
                        Ok(total_size) => {
                            debug!("Total size calculated successfully");

                            // Record calculate-total-size result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/calculate-total-size".to_string(),
                                data: StoreEventData::CalculateTotalSizeResult {                                    store_id: store_id.clone(),
                                    size: total_size,
                                    success: true,
                                }.into(),
                                                    description: Some(format!(
                                    "Successfully calculated total content size: {} bytes",
                                    total_size
                                )),
                            });

                            Ok((Ok(total_size),))
                        }
                        Err(e) => {
                            error!("Error calculating total content size: {}", e);

                            // Record calculate-total-size error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/calculate-total-size".to_string(),
                                data: StoreEventData::Error {                                    operation: "calculate-total-size".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!(
                                    "Error calculating total content size: {}",
                                    e
                                )),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: list-labels() - List all labels
        interface.func_wrap_async(
            "list-labels",
            move |mut ctx: StoreContextMut<'_, ActorStore<E>>, (store_id,): (String,)|
                -> Box<dyn Future<Output = Result<(Result<Vec<String>, String>,), anyhow::Error>> + Send> {
                // Record list labels call event
                ctx.data_mut().record_handler_event("theater:simple/store/list-labels".to_string(), StoreEventData::ListLabelsCall { store_id: store_id.clone() }, Some("Listing all labels".to_string()));

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.list_labels().await {
                        Ok(labels) => {
                            debug!("Labels listed successfully");

                            // Record list labels result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/list-labels".to_string(),
                                data: StoreEventData::ListLabelsResult {                                    store_id: store_id.clone(),
                                    labels: labels.clone(),
                                    success: true,
                                }.into(),
                                                    description: Some(format!("Successfully listed {} labels", labels.len())),
                            });

                            Ok((Ok(labels),))
                        },
                        Err(e) => {
                            error!("Error listing labels: {}", e);

                            // Record list labels error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/store/list-labels".to_string(),
                                data: StoreEventData::Error {                                    operation: "list-labels".to_string(),
                                    message: e.to_string(),
                                }.into(),
                                                    description: Some(format!("Error listing labels: {}", e)),
                            });

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "store-setup".to_string(),
            data: StoreEventData::HandlerSetupSuccess.into(),
            description: Some("Store host functions setup completed successfully".to_string()),
        });

        info!("Store host functions set up successfully");

        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance<E>,
    ) -> anyhow::Result<()> {
        info!("No export functions needed for store handler");
        Ok(())
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
        let cloned = handler.create_instance();

        assert_eq!(cloned.name(), "store");
    }
}
