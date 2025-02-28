use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::actor_store::ActorStore;
use crate::config::StoreHandlerConfig;
use crate::events::{ChainEventData, EventData};
use crate::events::store::StoreEventData;
use crate::store::ContentRef as RustContentRef;
use crate::store::ContentStore;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::component::{Lift, Lower, ComponentType};
use wasmtime::StoreContextMut;

#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower)]
#[component(record)]
struct ContentRefWit {
    hash: String,
}

impl From<RustContentRef> for ContentRefWit {
    fn from(cr: RustContentRef) -> Self {
        Self {
            hash: cr.hash().to_string(),
        }
    }
}

impl From<ContentRefWit> for RustContentRef {
    fn from(cr: ContentRefWit) -> Self {
        RustContentRef::new(cr.hash)
    }
}

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

pub struct StoreHost {
    store: ContentStore,
}

impl StoreHost {
    pub fn new(_config: StoreHandlerConfig, store: ContentStore) -> Self {
        Self { store }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up store host functions");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/store")
            .expect("could not instantiate ntwk:theater/store");

        // Store content function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "store",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (content,): (Vec<u8>,)| 
                -> Box<dyn Future<Output = Result<(Result<ContentRefWit, String>,)>> + Send> {
                // Record store call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/store".to_string(),
                    data: EventData::Store(StoreEventData::StoreCall {
                        content_size: content.len(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Storing {} bytes of content", content.len())),
                });
                
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Then perform the actual store operation (async)
                    match store.store(content).await {
                        Ok(content_ref) => {
                            debug!("Content stored successfully: {}", content_ref.hash());
                            
                            // Record store result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/store".to_string(),
                                data: EventData::Store(StoreEventData::StoreResult {
                                    hash: content_ref.hash().to_string(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Content stored successfully with hash: {}", content_ref.hash())),
                            });
                            
                            Ok((Ok(ContentRefWit::from(content_ref)),))
                        },
                        Err(e) => {
                            error!("Error storing content: {}", e);
                            
                            // Record store error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/store".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "store".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error storing content: {}", e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Get content function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "get",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (content_ref,): (ContentRefWit,)| 
                -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                // Record get call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/get".to_string(),
                    data: EventData::Store(StoreEventData::GetCall {
                        hash: content_ref.hash.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Getting content with hash: {}", content_ref.hash)),
                });
                
                let store = store_clone.clone();
                let hash = content_ref.hash.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.get(RustContentRef::from(content_ref)).await {
                        Ok(content) => {
                            debug!("Content retrieved successfully");
                            
                            // Record get result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/get".to_string(),
                                data: EventData::Store(StoreEventData::GetResult {
                                    hash: hash.clone(),
                                    content_size: content.len(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Retrieved {} bytes of content with hash: {}", content.len(), hash)),
                            });
                            
                            Ok((Ok(content),))
                        },
                        Err(e) => {
                            error!("Error retrieving content: {}", e);
                            
                            // Record get error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/get".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "get".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error retrieving content with hash {}: {}", hash, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Exists function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (content_ref,): (ContentRefWit,)| 
                -> Box<dyn Future<Output = Result<(Result<bool, String>,)>> + Send> {
                // Record exists call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/exists".to_string(),
                    data: EventData::Store(StoreEventData::ExistsCall {
                        hash: content_ref.hash.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Checking if content with hash {} exists", content_ref.hash)),
                });
                
                let store = store_clone.clone();
                let hash = content_ref.hash.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.exists(RustContentRef::from(content_ref)).await {
                        Ok(exists) => {
                            debug!("Content existence checked successfully");
                            
                            // Record exists result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/exists".to_string(),
                                data: EventData::Store(StoreEventData::ExistsResult {
                                    hash: hash.clone(),
                                    exists,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Content with hash {} exists: {}", hash, exists)),
                            });
                            
                            Ok((Ok(exists),))
                        },
                        Err(e) => {
                            error!("Error checking content existence: {}", e);
                            
                            // Record exists error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/exists".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "exists".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error checking if content with hash {} exists: {}", hash, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content_ref): (String, ContentRefWit)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                // Record label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/label".to_string(),
                    data: EventData::Store(StoreEventData::LabelCall {
                        label: label.clone(),
                        hash: content_ref.hash.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Labeling content with hash {} as '{}'", content_ref.hash, label)),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                let hash = content_ref.hash.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.label(label, RustContentRef::from(content_ref)).await {
                        Ok(_) => {
                            debug!("Content labeled successfully");
                            
                            // Record label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/label".to_string(),
                                data: EventData::Store(StoreEventData::LabelResult {
                                    label: label_clone.clone(),
                                    hash: hash.clone(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully labeled content with hash {} as '{}'", hash, label_clone)),
                            });
                            
                            Ok((Ok(()),))
                        },
                        Err(e) => {
                            error!("Error labeling content: {}", e);
                            
                            // Record label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error labeling content with hash {} as '{}': {}", hash, label_clone, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Get by label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "get-by-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label,): (String,)| 
                -> Box<dyn Future<Output = Result<(Result<Vec<ContentRefWit>, String>,)>> + Send> {
                // Record get-by-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/get-by-label".to_string(),
                    data: EventData::Store(StoreEventData::GetByLabelCall {
                        label: label.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Getting content references by label: {}", label)),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.get_by_label(label).await {
                        Ok(content_refs) => {
                            debug!("Content references by label retrieved successfully");
                            
                            // Convert the Rust ContentRef to ContentRefWit
                            let content_refs_wit: Vec<ContentRefWit> = content_refs
                                .into_iter()
                                .map(ContentRefWit::from)
                                .collect();
                            
                            // Record get-by-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/get-by-label".to_string(),
                                data: EventData::Store(StoreEventData::GetByLabelResult {
                                    label: label_clone.clone(),
                                    refs_count: content_refs_wit.len(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully retrieved {} content references for label '{}'", content_refs_wit.len(), label_clone)),
                            });
                            
                            Ok((Ok(content_refs_wit),))
                        },
                        Err(e) => {
                            error!("Error retrieving content references by label: {}", e);
                            
                            // Record get-by-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/get-by-label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "get-by-label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error retrieving content references for label '{}': {}", label_clone, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Remove label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "remove-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label,): (String,)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                // Record remove-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/remove-label".to_string(),
                    data: EventData::Store(StoreEventData::RemoveLabelCall {
                        label: label.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Removing label: {}", label)),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.remove_label(label).await {
                        Ok(_) => {
                            debug!("Label removed successfully");
                            
                            // Record remove-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/remove-label".to_string(),
                                data: EventData::Store(StoreEventData::RemoveLabelResult {
                                    label: label_clone.clone(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully removed label '{}'", label_clone)),
                            });
                            
                            Ok((Ok(()),))
                        },
                        Err(e) => {
                            error!("Error removing label: {}", e);
                            
                            // Record remove-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/remove-label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "remove-label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error removing label '{}': {}", label_clone, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Remove from label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "remove-from-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content_ref): (String, ContentRefWit)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                // Record remove-from-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/remove-from-label".to_string(),
                    data: EventData::Store(StoreEventData::RemoveFromLabelCall {
                        label: label.clone(),
                        hash: content_ref.hash.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Removing content with hash {} from label: {}", content_ref.hash, label)),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                let hash = content_ref.hash.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.remove_from_label(label, RustContentRef::from(content_ref)).await {
                        Ok(_) => {
                            debug!("Content removed from label successfully");
                            
                            // Record remove-from-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/remove-from-label".to_string(),
                                data: EventData::Store(StoreEventData::RemoveFromLabelResult {
                                    label: label_clone.clone(),
                                    hash: hash.clone(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully removed content with hash {} from label '{}'", hash, label_clone)),
                            });
                            
                            Ok((Ok(()),))
                        },
                        Err(e) => {
                            error!("Error removing content from label: {}", e);
                            
                            // Record remove-from-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/remove-from-label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "remove-from-label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error removing content with hash {} from label '{}': {}", hash, label_clone, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Put at label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "put-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content): (String, Vec<u8>)| 
                -> Box<dyn Future<Output = Result<(Result<ContentRefWit, String>,)>> + Send> {
                // Record put-at-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/put-at-label".to_string(),
                    data: EventData::Store(StoreEventData::PutAtLabelCall {
                        label: label.clone(),
                        content_size: content.len(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Storing {} bytes of content at label: {}", content.len(), label)),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.put_at_label(label, content).await {
                        Ok(content_ref) => {
                            debug!("Content stored at label successfully");
                            let content_ref_wit = ContentRefWit::from(content_ref.clone());
                            
                            // Record put-at-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/put-at-label".to_string(),
                                data: EventData::Store(StoreEventData::PutAtLabelResult {
                                    label: label_clone.clone(),
                                    hash: content_ref.hash().to_string(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully stored content with hash {} at label '{}'", content_ref.hash(), label_clone)),
                            });
                            
                            Ok((Ok(content_ref_wit),))
                        },
                        Err(e) => {
                            error!("Error storing content at label: {}", e);
                            
                            // Record put-at-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/put-at-label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "put-at-label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error storing content at label '{}': {}", label_clone, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Replace content at label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "replace-content-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content): (String, Vec<u8>)| 
                -> Box<dyn Future<Output = Result<(Result<ContentRefWit, String>,)>> + Send> {
                // Record replace-content-at-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/replace-content-at-label".to_string(),
                    data: EventData::Store(StoreEventData::ReplaceContentAtLabelCall {
                        label: label.clone(),
                        content_size: content.len(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Replacing content at label {} with {} bytes of new content", label, content.len())),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.replace_content_at_label(label, content).await {
                        Ok(content_ref) => {
                            debug!("Content at label replaced successfully");
                            let content_ref_wit = ContentRefWit::from(content_ref.clone());
                            
                            // Record replace-content-at-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/replace-content-at-label".to_string(),
                                data: EventData::Store(StoreEventData::ReplaceContentAtLabelResult {
                                    label: label_clone.clone(),
                                    hash: content_ref.hash().to_string(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully replaced content at label '{}' with new content (hash: {})", label_clone, content_ref.hash())),
                            });
                            
                            Ok((Ok(content_ref_wit),))
                        },
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);
                            
                            // Record replace-content-at-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/replace-content-at-label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "replace-content-at-label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error replacing content at label '{}': {}", label_clone, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Replace at label function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "replace-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content_ref): (String, ContentRefWit)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                // Record replace-at-label call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/replace-at-label".to_string(),
                    data: EventData::Store(StoreEventData::ReplaceAtLabelCall {
                        label: label.clone(),
                        hash: content_ref.hash.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Replacing content at label {} with content reference: {}", label, content_ref.hash)),
                });
                
                let store = store_clone.clone();
                let label_clone = label.clone();
                let hash = content_ref.hash.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.replace_at_label(label, RustContentRef::from(content_ref)).await {
                        Ok(_) => {
                            debug!("Content at label replaced with reference successfully");
                            
                            // Record replace-at-label result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/replace-at-label".to_string(),
                                data: EventData::Store(StoreEventData::ReplaceAtLabelResult {
                                    label: label_clone.clone(),
                                    hash: hash.clone(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully replaced content at label '{}' with content reference (hash: {})", label_clone, hash)),
                            });
                            
                            Ok((Ok(()),))
                        },
                        Err(e) => {
                            error!("Error replacing content at label with reference: {}", e);
                            
                            // Record replace-at-label error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/replace-at-label".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "replace-at-label".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error replacing content at label '{}' with reference (hash: {}): {}", label_clone, hash, e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // List all content function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "list-all-content",
            move |mut ctx: StoreContextMut<'_, ActorStore>, ()| 
                -> Box<dyn Future<Output = Result<(Result<Vec<ContentRefWit>, String>,)>> + Send> {
                // Record list-all-content call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/list-all-content".to_string(),
                    data: EventData::Store(StoreEventData::ListAllContentCall {}),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Listing all content references".to_string()),
                });
                
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.list_all_content().await {
                        Ok(content_refs) => {
                            debug!("All content references listed successfully");
                            
                            // Convert the Rust ContentRef to ContentRefWit
                            let content_refs_wit: Vec<ContentRefWit> = content_refs
                                .into_iter()
                                .map(ContentRefWit::from)
                                .collect();
                            
                            // Record list-all-content result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/list-all-content".to_string(),
                                data: EventData::Store(StoreEventData::ListAllContentResult {
                                    refs_count: content_refs_wit.len(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully listed {} content references", content_refs_wit.len())),
                            });
                            
                            Ok((Ok(content_refs_wit),))
                        },
                        Err(e) => {
                            error!("Error listing all content references: {}", e);
                            
                            // Record list-all-content error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/list-all-content".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "list-all-content".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error listing all content references: {}", e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // Calculate total size function
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "calculate-total-size",
            move |mut ctx: StoreContextMut<'_, ActorStore>, ()| 
                -> Box<dyn Future<Output = Result<(Result<u64, String>,)>> + Send> {
                // Record calculate-total-size call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/calculate-total-size".to_string(),
                    data: EventData::Store(StoreEventData::CalculateTotalSizeCall {}),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Calculating total size of all content".to_string()),
                });
                
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.calculate_total_size().await {
                        Ok(total_size) => {
                            debug!("Total size calculated successfully");
                            
                            // Record calculate-total-size result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/calculate-total-size".to_string(),
                                data: EventData::Store(StoreEventData::CalculateTotalSizeResult {
                                    size: total_size,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully calculated total content size: {} bytes", total_size)),
                            });
                            
                            Ok((Ok(total_size),))
                        },
                        Err(e) => {
                            error!("Error calculating total content size: {}", e);
                            
                            // Record calculate-total-size error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/calculate-total-size".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "calculate-total-size".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error calculating total content size: {}", e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // List labels function as an example of a function with no input parameters
        let store_clone = self.store.clone();

        interface.func_wrap_async(
            "list-labels",
            move |mut ctx: StoreContextMut<'_, ActorStore>, ()| 
                -> Box<dyn Future<Output = Result<(Result<Vec<String>, String>,)>> + Send> {
                // Record list labels call event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "ntwk:theater/store/list-labels".to_string(),
                    data: EventData::Store(StoreEventData::ListLabelsCall {}),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Listing all labels".to_string()),
                });
                
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Perform the operation
                    match store.list_labels().await {
                        Ok(labels) => {
                            debug!("Labels listed successfully");
                            
                            // Record list labels result event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/list-labels".to_string(),
                                data: EventData::Store(StoreEventData::ListLabelsResult {
                                    labels_count: labels.len(),
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully listed {} labels", labels.len())),
                            });
                            
                            Ok((Ok(labels),))
                        },
                        Err(e) => {
                            error!("Error listing labels: {}", e);
                            
                            // Record list labels error event
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/store/list-labels".to_string(),
                                data: EventData::Store(StoreEventData::Error {
                                    operation: "list-labels".to_string(),
                                    message: e.to_string(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error listing labels: {}", e)),
                            });
                            
                            Ok((Err(e.to_string()),))
                        }
                    }
                })
            },
        )?;

        // You would implement all the other store functions following the same pattern

        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No functions needed for store");
        Ok(())
    }

    pub async fn start(&self, _actor_handle: ActorHandle) -> Result<()> {
        info!("STORE handler starting...");
        Ok(())
    }
}
