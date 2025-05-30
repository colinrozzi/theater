//! # Random Number Generator Host
//!
//! Provides random number generation capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to generate random bytes, integers within ranges, and floating-point
//! numbers while maintaining security boundaries and resource limits.

use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::RandomHandlerConfig;
use crate::events::{random::RandomEventData, ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use rand::prelude::*;
use rand_chacha::ChaCha20Rng;
use std::sync::{Arc, Mutex};
use std::future::Future;
use tracing::info;

/// Host for providing random number generation capabilities to WebAssembly actors
pub struct RandomHost {
    config: RandomHandlerConfig,
    rng: Arc<Mutex<ChaCha20Rng>>,
}

/// Error types for random operations
#[derive(Debug, thiserror::Error)]
pub enum RandomError {
    #[error("Random generation error: {0}")]
    GenerationError(String),
    
    #[error("Invalid range: min ({0}) >= max ({1})")]
    InvalidRange(u64, u64),
    
    #[error("Requested bytes ({0}) exceeds maximum allowed ({1})")]
    TooManyBytes(usize, usize),
    
    #[error("Requested max value ({0}) exceeds configured maximum ({1})")]
    ValueTooLarge(u64, u64),
}

impl RandomHost {
    pub fn new(config: RandomHandlerConfig) -> Self {
        let rng = if let Some(seed) = config.seed {
            info!("Initializing random host with seed: {}", seed);
            Arc::new(Mutex::new(ChaCha20Rng::seed_from_u64(seed)))
        } else {
            info!("Initializing random host with entropy from OS");
            Arc::new(Mutex::new(ChaCha20Rng::from_entropy()))
        };

        Self { config, rng }
    }

    pub async fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Result<()> {
        info!("Setting up random number generator host functions");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/random")
            .expect("Could not instantiate ntwk:theater/random");

        let rng = Arc::clone(&self.rng);
        let config = self.config.clone();

        // Generate random bytes
        interface.func_wrap_async(
            "random-bytes",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (size,): (u32,)| -> Box<dyn Future<Output = Result<(Result<Vec<u8>, String>,)>> + Send> {
                let rng = Arc::clone(&rng);
                let config = config.clone();
                
                Box::new(async move {
                    let size = size as usize;
                    
                    // Record the random bytes call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/random-host/random-bytes".to_string(),
                        data: EventData::Random(RandomEventData::RandomBytesCall {
                            requested_size: size,
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Generating {} random bytes", size)),
                    });

                    if size > config.max_bytes {
                        let error_msg = format!("Requested {} bytes exceeds maximum {}", size, config.max_bytes);
                        
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/random-host/random-bytes".to_string(),
                            data: EventData::Random(RandomEventData::Error {
                                operation: "random-bytes".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Error generating random bytes: {}", error_msg)),
                        });
                        
                        return Ok((Err(error_msg),));
                    }

                    let mut bytes = vec![0u8; size];
                    match rng.lock() {
                        Ok(mut generator) => {
                            generator.fill_bytes(&mut bytes);
                            
                            // Record successful result
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/random-host/random-bytes".to_string(),
                                data: EventData::Random(RandomEventData::RandomBytesResult {
                                    generated_size: size,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully generated {} random bytes", size)),
                            });
                            
                            Ok((Ok(bytes),))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to acquire RNG lock: {}", e);
                            
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/random-host/random-bytes".to_string(),
                                data: EventData::Random(RandomEventData::Error {
                                    operation: "random-bytes".to_string(),
                                    message: error_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error generating random bytes: {}", error_msg)),
                            });
                            
                            Ok((Err(error_msg),))
                        }
                    }
                })
            },
        )?;

        let rng = Arc::clone(&self.rng);
        let config = self.config.clone();

        // Generate random integer in range
        interface.func_wrap_async(
            "random-range",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (min, max): (u64, u64)| -> Box<dyn Future<Output = Result<(Result<u64, String>,)>> + Send> {
                let rng = Arc::clone(&rng);
                let config = config.clone();
                
                Box::new(async move {
                    // Record the random range call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/random-host/random-range".to_string(),
                        data: EventData::Random(RandomEventData::RandomRangeCall {
                            min,
                            max
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Generating random number in range {} to {}", min, max)),
                    });

                    if min >= max {
                        let error_msg = format!("Invalid range: min ({}) >= max ({})", min, max);
                        
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/random-host/random-range".to_string(),
                            data: EventData::Random(RandomEventData::Error {
                                operation: "random-range".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Error generating random range: {}", error_msg)),
                        });
                        
                        return Ok((Err(error_msg),));
                    }

                    if max > config.max_int {
                        let error_msg = format!("Requested max value ({}) exceeds configured maximum ({})", max, config.max_int);
                        
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/random-host/random-range".to_string(),
                            data: EventData::Random(RandomEventData::Error {
                                operation: "random-range".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Error generating random range: {}", error_msg)),
                        });
                        
                        return Ok((Err(error_msg),));
                    }

                    match rng.lock() {
                        Ok(mut generator) => {
                            let value = generator.gen_range(min..max);
                            
                            // Record successful result
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/random-host/random-range".to_string(),
                                data: EventData::Random(RandomEventData::RandomRangeResult {
                                    min,
                                    max,
                                    value,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully generated random number {} in range {} to {}", value, min, max)),
                            });
                            
                            Ok((Ok(value),))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to acquire RNG lock: {}", e);
                            
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/random-host/random-range".to_string(),
                                data: EventData::Random(RandomEventData::Error {
                                    operation: "random-range".to_string(),
                                    message: error_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error generating random range: {}", error_msg)),
                            });
                            
                            Ok((Err(error_msg),))
                        }
                    }
                })
            },
        )?;

        let rng = Arc::clone(&self.rng);

        // Generate random float between 0.0 and 1.0
        interface.func_wrap_async(
            "random-float",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (): ()| -> Box<dyn Future<Output = Result<(Result<f64, String>,)>> + Send> {
                let rng = Arc::clone(&rng);
                
                Box::new(async move {
                    // Record the random float call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/random-host/random-float".to_string(),
                        data: EventData::Random(RandomEventData::RandomFloatCall),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some("Generating random float between 0.0 and 1.0".to_string()),
                    });

                    match rng.lock() {
                        Ok(mut generator) => {
                            let value: f64 = generator.gen();
                            
                            // Record successful result
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/random-host/random-float".to_string(),
                                data: EventData::Random(RandomEventData::RandomFloatResult {
                                    value,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Successfully generated random float: {}", value)),
                            });
                            
                            Ok((Ok(value),))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to acquire RNG lock: {}", e);
                            
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "ntwk:theater/random-host/random-float".to_string(),
                                data: EventData::Random(RandomEventData::Error {
                                    operation: "random-float".to_string(),
                                    message: error_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!("Error generating random float: {}", error_msg)),
                            });
                            
                            Ok((Err(error_msg),))
                        }
                    }
                })
            },
        )?;

        let rng = Arc::clone(&self.rng);
        interface
            .func_wrap_async(
                "generate-uuid",
                move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                      (): ()| -> Box<dyn Future<Output = Result<(Result<String, String>,)>> + Send> {
                    let rng = Arc::clone(&rng);
                    
                    Box::new(async move {
                        // Record the UUID generation call event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/random-host/generate-uuid".to_string(),
                            data: EventData::Random(RandomEventData::GenerateUuidCall),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some("Generating random UUID".to_string()),
                        });

                        match rng.lock() {
                            Ok(_generator) => {
                                let uuid = uuid::Uuid::new_v4();
                                let uuid_str = uuid.to_string();
                                
                                // Record successful result
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/random-host/generate-uuid".to_string(),
                                    data: EventData::Random(RandomEventData::GenerateUuidResult {
                                        uuid: uuid_str.clone(),
                                        success: true,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Successfully generated UUID: {}", uuid_str)),
                                });
                                
                                Ok((Ok(uuid_str),))
                            }
                            Err(e) => {
                                let error_msg = format!("Failed to acquire RNG lock: {}", e);
                                
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "ntwk:theater/random-host/generate-uuid".to_string(),
                                    data: EventData::Random(RandomEventData::Error {
                                        operation: "generate-uuid".to_string(),
                                        message: error_msg.clone(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Error generating UUID: {}", error_msg)),
                                });
                                
                                Ok((Err(error_msg),))
                            }
                        }
                    })
                },
            )?;

        info!("Random number generator host functions setup complete");
        Ok(())
    }

    pub async fn start(
        &mut self,
        _actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting random number generator host");

        // Random host doesn't need a background task, but we should wait for shutdown
        shutdown_receiver.wait_for_shutdown().await;
        info!("Random host received shutdown signal");

        info!("Random host shut down");
        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        // Random host doesn't export functions to actors, only provides host functions
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RandomHandlerConfig;
    
    #[test] 
    fn test_random_handler_config_defaults() {
        let config = RandomHandlerConfig {
            seed: None,
            max_bytes: 1024,
            max_int: 1000,
            allow_crypto_secure: false,
        };
        
        let handler = RandomHost::new(config.clone());
        assert_eq!(handler.config.max_bytes, 1024);
        assert_eq!(handler.config.max_int, 1000);
        assert_eq!(handler.config.allow_crypto_secure, false);
        assert!(handler.config.seed.is_none());
    }
    
    #[test]
    fn test_random_handler_with_seed() {
        let config = RandomHandlerConfig {
            seed: Some(12345),
            max_bytes: 1024 * 1024,
            max_int: u64::MAX,
            allow_crypto_secure: false,
        };
        
        let handler = RandomHost::new(config);
        assert_eq!(handler.config.seed, Some(12345));
    }
}
