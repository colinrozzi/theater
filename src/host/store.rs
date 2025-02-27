use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::StoreHandlerConfig;
use crate::host::host_wrapper::HostFunctionBoundary;
use crate::store::ContentRef as RustContentRef;
use crate::store::ContentStore;
use crate::wasm::{ActorComponent, ActorInstance};
use crate::actor_store::ActorStore;
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
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "store");

        interface.func_wrap_async(
            "store",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (content,): (Vec<u8>,)| 
                -> Box<dyn Future<Output = Result<(Result<ContentRefWit, String>,)>> + Send> {
                // Record the operation input
                let content_clone = content.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // First wrap the input recording (sync)
                    boundary.wrap(&mut ctx, content_clone, |_| Ok(()))?;
                    
                    // Then perform the actual store operation (async)
                    let result = match store.store(content).await {
                        Ok(content_ref) => {
                            debug!("Content stored successfully: {}", content_ref.hash());
                            Ok(ContentRefWit::from(content_ref))
                        },
                        Err(e) => {
                            error!("Error storing content: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Finally wrap the output recording (sync)
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Get content function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "get");

        interface.func_wrap_async(
            "get",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (content_ref,): (ContentRefWit,)| 
                -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                let content_ref_clone = content_ref.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, content_ref_clone, |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.get(RustContentRef::from(content_ref)).await {
                        Ok(content) => {
                            debug!("Content retrieved successfully");
                            Ok(content)
                        },
                        Err(e) => {
                            error!("Error retrieving content: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Exists function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "exists");

        interface.func_wrap_async(
            "exists",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (content_ref,): (ContentRefWit,)| 
                -> Box<dyn Future<Output = Result<(Result<bool, String>,)>> + Send> {
                let content_ref_clone = content_ref.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, content_ref_clone, |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.exists(RustContentRef::from(content_ref)).await {
                        Ok(exists) => {
                            debug!("Content existence checked successfully");
                            Ok(exists)
                        },
                        Err(e) => {
                            error!("Error checking content existence: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "label");

        interface.func_wrap_async(
            "label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content_ref): (String, ContentRefWit)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let label_clone = label.clone();
                let content_ref_clone = content_ref.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (label_clone, content_ref_clone), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.label(label, RustContentRef::from(content_ref)).await {
                        Ok(_) => {
                            debug!("Content labeled successfully");
                            Ok(())
                        },
                        Err(e) => {
                            error!("Error labeling content: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Get by label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "get-by-label");

        interface.func_wrap_async(
            "get-by-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label,): (String,)| 
                -> Box<dyn Future<Output = Result<(Result<Vec<ContentRefWit>, String>,)>> + Send> {
                let label_clone = label.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, label_clone, |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.get_by_label(label).await {
                        Ok(refs) => {
                            let wit_refs: Vec<ContentRefWit> = refs.into_iter()
                                .map(ContentRefWit::from)
                                .collect();
                            debug!("Content references by label retrieved successfully");
                            Ok(wit_refs)
                        },
                        Err(e) => {
                            error!("Error retrieving content references by label: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Remove label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "remove-label");

        interface.func_wrap_async(
            "remove-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label,): (String,)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let label_clone = label.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, label_clone, |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.remove_label(label).await {
                        Ok(_) => {
                            debug!("Label removed successfully");
                            Ok(())
                        },
                        Err(e) => {
                            error!("Error removing label: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Remove from label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "remove-from-label");

        interface.func_wrap_async(
            "remove-from-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content_ref): (String, ContentRefWit)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let label_clone = label.clone();
                let content_ref_clone = content_ref.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (label_clone, content_ref_clone), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.remove_from_label(label, RustContentRef::from(content_ref)).await {
                        Ok(_) => {
                            debug!("Content reference removed from label successfully");
                            Ok(())
                        },
                        Err(e) => {
                            error!("Error removing content reference from label: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Put at label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "put-at-label");

        interface.func_wrap_async(
            "put-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content): (String, Vec<u8>)| 
                -> Box<dyn Future<Output = Result<(Result<ContentRefWit, String>,)>> + Send> {
                let label_clone = label.clone();
                let content_clone = content.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (label_clone, content_clone), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.put_at_label(label, content).await {
                        Ok(content_ref) => {
                            debug!("Content stored and labeled successfully");
                            Ok(ContentRefWit::from(content_ref))
                        },
                        Err(e) => {
                            error!("Error storing and labeling content: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Replace content at label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "replace-content-at-label");

        interface.func_wrap_async(
            "replace-content-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content): (String, Vec<u8>)| 
                -> Box<dyn Future<Output = Result<(Result<ContentRefWit, String>,)>> + Send> {
                let label_clone = label.clone();
                let content_clone = content.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (label_clone, content_clone), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.replace_content_at_label(label, content).await {
                        Ok(content_ref) => {
                            debug!("Content replaced at label successfully");
                            Ok(ContentRefWit::from(content_ref))
                        },
                        Err(e) => {
                            error!("Error replacing content at label: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Replace at label function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "replace-at-label");

        interface.func_wrap_async(
            "replace-at-label",
            move |mut ctx: StoreContextMut<'_, ActorStore>, (label, content_ref): (String, ContentRefWit)| 
                -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let label_clone = label.clone();
                let content_ref_clone = content_ref.clone();
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (label_clone, content_ref_clone), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.replace_at_label(label, RustContentRef::from(content_ref)).await {
                        Ok(_) => {
                            debug!("Content reference replaced at label successfully");
                            Ok(())
                        },
                        Err(e) => {
                            error!("Error replacing content reference at label: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // List labels function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "list-labels");

        interface.func_wrap_async(
            "list-labels",
            move |mut ctx: StoreContextMut<'_, ActorStore>, ()| 
                -> Box<dyn Future<Output = Result<(Result<Vec<String>, String>,)>> + Send> {
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.list_labels().await {
                        Ok(labels) => {
                            debug!("Labels listed successfully");
                            Ok(labels)
                        },
                        Err(e) => {
                            error!("Error listing labels: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // List all content function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "list-all-content");

        interface.func_wrap_async(
            "list-all-content",
            move |mut ctx: StoreContextMut<'_, ActorStore>, ()| 
                -> Box<dyn Future<Output = Result<(Result<Vec<ContentRefWit>, String>,)>> + Send> {
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.list_all_content().await {
                        Ok(refs) => {
                            let wit_refs: Vec<ContentRefWit> = refs.into_iter()
                                .map(ContentRefWit::from)
                                .collect();
                            debug!("All content references listed successfully");
                            Ok(wit_refs)
                        },
                        Err(e) => {
                            error!("Error listing all content references: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

        // Calculate total size function
        let store_clone = self.store.clone();
        let boundary = HostFunctionBoundary::new("ntwk:theater/store", "calculate-total-size");

        interface.func_wrap_async(
            "calculate-total-size",
            move |mut ctx: StoreContextMut<'_, ActorStore>, ()| 
                -> Box<dyn Future<Output = Result<(Result<u64, String>,)>> + Send> {
                let boundary = boundary.clone();
                let store = store_clone.clone();
                
                Box::new(async move {
                    // Record the input
                    boundary.wrap(&mut ctx, (), |_| Ok(()))?;
                    
                    // Perform the operation
                    let result = match store.calculate_total_size().await {
                        Ok(size) => {
                            debug!("Total size calculated successfully: {}", size);
                            Ok(size)
                        },
                        Err(e) => {
                            error!("Error calculating total size: {}", e);
                            Err(e.to_string())
                        }
                    };
                    
                    // Record the output
                    boundary.wrap(&mut ctx, result.clone(), |result| Ok((result,)))
                })
            },
        )?;

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
