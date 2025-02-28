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

        // Implement other store functions following the same pattern...
        // For brevity I'm only implementing a subset of the functions - you would continue with the same pattern for all others

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
