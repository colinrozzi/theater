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
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::actor_manifest::StoreHandlerConfig;
use theater::config::permissions::StorePermissions;
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

impl Handler for StoreHandler
{
    fn create_instance(&self) -> Box<dyn Handler> {
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

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
        _ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        // Record setup start

        info!("Setting up store host functions");

        let mut interface = match actor_component.linker.instance("theater:simple/store") {
            Ok(interface) => {
                // Record successful linker instance creation
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/store: {}",
                    e
                ));
            }
        };

        // Setup: new() - Create a new content store
        interface.func_wrap(
            "new",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (): ()| -> Result<(Result<String, String>,), anyhow::Error> {
                // Record store call event
                
                let store = ContentStore::new();

                // Record store result event

                Ok((Ok(store.id().to_string()),))
            },
        )?;

        // Setup: store() - Store content
        interface.func_wrap_async(
            "store",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (store_id, content): (String, Vec<u8>)|
                -> Box<dyn Future<Output = Result<(Result<ContentRef, String>,), anyhow::Error>> + Send> {
                // Record store call event

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the actual store operation (async)
                    let content_ref = store.store(content).await;
                    debug!("Content stored successfully: {}", content_ref.hash());

                    // Record store result event

                    Ok((Ok(ContentRef::from(content_ref)),))
                })
            },
        )?;

        // Setup: get() - Retrieve content
        interface.func_wrap_async(
            "get",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (store_id, content_ref): (String, ContentRef)|
                -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,), anyhow::Error>> + Send> {
                // Record get call event

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.get(&content_ref).await {
                        Ok(content) => {
                            debug!("Content retrieved successfully");

                            // Record get result event

                            Ok((Ok(content),))
                        },
                        Err(e) => {
                            error!("Error retrieving content: {}", e);

                            // Record get error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: exists() - Check if content exists
        interface.func_wrap_async(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (store_id, content_ref): (String, ContentRef)|
                  -> Box<dyn Future<Output = Result<(Result<bool, String>,), anyhow::Error>> + Send> {
                // Record exists call event

                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    let exists = store.exists(&content_ref).await;
                    debug!("Content existence checked successfully");

                    // Record exists result event

                    Ok((Ok(exists),))
                })
            },
        )?;

        // Setup: label() - Add a label to content
        interface.func_wrap_async(
            "label",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (store_id, label_string, content_ref): (String, String, ContentRef)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,), anyhow::Error>> + Send> {
                // Record label call event

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.label(&label, &content_ref).await {
                        Ok(_) => {
                            debug!("Content labeled successfully");

                            // Record label result event

                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            error!("Error labeling content: {}", e);

                            // Record label error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: get-by-label() - Get content reference by label
        interface.func_wrap_async(
            "get-by-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (store_id, label_string): (String, String)|
                  -> Box<dyn Future<Output = Result<(Result<Option<ContentRef>, String>,), anyhow::Error>> + Send> {
                // Record get-by-label call event

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.get_by_label(&label).await {
                        Ok(content_ref_opt) => {
                            debug!("Content reference by label retrieved successfully");

                            // Record get-by-label result event

                            Ok((Ok(content_ref_opt),))
                        }
                        Err(e) => {
                            error!("Error retrieving content reference by label: {}", e);

                            // Record get-by-label error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: remove-label() - Remove a label
        interface.func_wrap_async(
            "remove-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (store_id, label_string): (String, String)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,), anyhow::Error>> + Send> {
                // Record remove-label call event

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.remove_label(&label).await {
                        Ok(_) => {
                            debug!("Label removed successfully");

                            // Record remove-label result event

                            Ok((Ok(()),))
                        }
                        Err(e) => {
                            error!("Error removing label: {}", e);

                            // Record remove-label error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: store-at-label() - Store content and label it
        interface.func_wrap_async(
            "store-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (store_id, label_string, content): (String, String, Vec<u8>)|
                -> Box<dyn Future<Output = Result<(Result<ContentRef, String>,), anyhow::Error>> + Send> {
                // Record store-at-label call event
                
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

                            Ok((Ok(content_ref_wit),))
                        },
                        Err(e) => {
                            error!("Error storing content at label [{}]: {}", label, e);

                            // Record store-at-label error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: replace-content-at-label() - Replace content at a label
        interface.func_wrap_async(
            "replace-content-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (store_id, label_string, content): (String, String, Vec<u8>)|
                -> Box<dyn Future<Output = Result<(Result<ContentRef, String>,), anyhow::Error>> + Send> {
                // Record replace-content-at-label call event

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

                            Ok((Ok(content_ref_wit),))
                        },
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);

                            // Record replace-content-at-label error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: replace-at-label() - Replace content reference at a label
        interface.func_wrap_async(
            "replace-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (store_id, label_string, content_ref): (String, String, ContentRef)|
                -> Box<dyn Future<Output = Result<(Result<(), String>,), anyhow::Error>> + Send> {
                // Record replace-at-label call event

                let store = ContentStore::from_id(&store_id);
                let label = Label::new(label_string.clone());
                let label_clone = label.clone();

                Box::new(async move {
                    // Perform the operation
                    match store.replace_at_label(&label, &content_ref).await {
                        Ok(_) => {
                            debug!("Content at label replaced with reference successfully");

                            // Record replace-at-label result event

                            Ok((Ok(()),))
                        },
                        Err(e) => {
                            error!("Error replacing content at label with reference: {}", e);

                            // Record replace-at-label error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: list-all-content() - List all content references
        interface.func_wrap_async(
            "list-all-content",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (store_id,): (String,)|
                  -> Box<dyn Future<Output = Result<(Result<Vec<ContentRef>, String>,), anyhow::Error>> + Send> {
                // Record list-all-content call event
                
                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.list_all_content().await {
                        Ok(content_refs) => {
                            debug!("All content references listed successfully");

                            // Record list-all-content result event

                            Ok((Ok(content_refs),))
                        }
                        Err(e) => {
                            error!("Error listing all content references: {}", e);

                            // Record list-all-content error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: calculate-total-size() - Calculate total size of all content
        interface.func_wrap_async(
            "calculate-total-size",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (store_id,): (String,)|
                  -> Box<dyn Future<Output = Result<(Result<u64, String>,), anyhow::Error>> + Send> {
                // Record calculate-total-size call event
                
                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.calculate_total_size().await {
                        Ok(total_size) => {
                            debug!("Total size calculated successfully");

                            // Record calculate-total-size result event

                            Ok((Ok(total_size),))
                        }
                        Err(e) => {
                            error!("Error calculating total content size: {}", e);

                            // Record calculate-total-size error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Setup: list-labels() - List all labels
        interface.func_wrap_async(
            "list-labels",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (store_id,): (String,)|
                -> Box<dyn Future<Output = Result<(Result<Vec<String>, String>,), anyhow::Error>> + Send> {
                // Record list labels call event
                
                let store = ContentStore::from_id(&store_id);

                Box::new(async move {
                    // Perform the operation
                    match store.list_labels().await {
                        Ok(labels) => {
                            debug!("Labels listed successfully");

                            // Record list labels result event

                            Ok((Ok(labels),))
                        },
                        Err(e) => {
                            error!("Error listing labels: {}", e);

                            // Record list labels error event

                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Record overall setup completion

        info!("Store host functions set up successfully");

        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance,
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
