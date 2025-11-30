//! # Random Number Generator Handler
//!
//! Provides random number generation capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to generate random bytes, integers within ranges, and floating-point
//! numbers while maintaining security boundaries and resource limits.

use rand::prelude::*;
use rand_chacha::ChaCha20Rng;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tracing::info;
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::actor_manifest::RandomHandlerConfig;
use theater::config::enforcement::PermissionChecker;
use theater::config::permissions::RandomPermissions;
use theater::events::{random::RandomEventData, ChainEventData, EventData};
use theater::handler::Handler;
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

/// Host for providing random number generation capabilities to WebAssembly actors
pub struct RandomHandler {
    config: RandomHandlerConfig,
    rng: Arc<Mutex<ChaCha20Rng>>,
    permissions: Option<RandomPermissions>,
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

impl RandomHandler {
    pub fn new(config: RandomHandlerConfig, permissions: Option<RandomPermissions>) -> Self {
        let rng = if let Some(seed) = config.seed {
            info!("Initializing random handler with seed: {}", seed);
            Arc::new(Mutex::new(ChaCha20Rng::seed_from_u64(seed)))
        } else {
            info!("Initializing random handler with entropy from OS");
            Arc::new(Mutex::new(ChaCha20Rng::from_entropy()))
        };

        Self {
            config,
            rng,
            permissions,
        }
    }
}

impl Handler for RandomHandler {
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(Self::new(self.config.clone(), self.permissions.clone()))
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Starting random number generator handler");

        Box::pin(async {
            // Random handler doesn't need a background task, but we should wait for shutdown
            shutdown_receiver.wait_for_shutdown().await;
            info!("Random handler received shutdown signal");
            info!("Random handler shut down");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> anyhow::Result<()> {
        // Clone what we need for the closures
        let rng1 = Arc::clone(&self.rng);
        let config1 = self.config.clone();
        let permissions1 = self.permissions.clone();
        
        let rng2 = Arc::clone(&self.rng);
        let config2 = self.config.clone();
        
        let rng3 = Arc::clone(&self.rng);
        let rng4 = Arc::clone(&self.rng);
        
        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "random-setup".to_string(),
            data: EventData::Random(RandomEventData::HandlerSetupStart),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Starting random host function setup".to_string()),
        });

        info!("Setting up random number generator host functions");

        let mut interface = match actor_component.linker.instance("theater:simple/random") {
            Ok(interface) => {
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "random-setup".to_string(),
                    data: EventData::Random(RandomEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "random-setup".to_string(),
                    data: EventData::Random(RandomEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/random: {}",
                    e
                ));
            }
        };

        // Generate random bytes
        interface.func_wrap_async(
            "random-bytes",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (size,): (u32,)| -> Box<dyn Future<Output = anyhow::Result<(Result<Vec<u8>, String>,)>> + Send> {
                let rng = Arc::clone(&rng1);
                let _config = config1.clone();
                let permissions = permissions1.clone();
                
                Box::new(async move {
                    let size = size as usize;
                    
                    // Record the random bytes call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/random-host/random-bytes".to_string(),
                        data: EventData::Random(RandomEventData::RandomBytesCall {
                            requested_size: size,
                        }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!("Generating {} random bytes", size)),
                    });

                    // PERMISSION CHECK BEFORE OPERATION
                    if let Err(e) = PermissionChecker::check_random_operation(
                        &permissions,
                        "random-bytes",
                        Some(size),
                        None,
                    ) {
                        // Record permission denied event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/random-host/permission-denied".to_string(),
                            data: EventData::Random(RandomEventData::PermissionDenied {
                                operation: "random-bytes".to_string(),
                                reason: e.to_string(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Permission denied for random bytes generation: {}", e)),
                        });
                        
                        return Ok((Err(format!("Permission denied: {}", e)),));
                    }

                    let mut bytes = vec![0u8; size];
                    match rng.lock() {
                        Ok(mut generator) => {
                            generator.fill_bytes(&mut bytes);
                            
                            // Record successful result
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/random-host/random-bytes".to_string(),
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
                                event_type: "theater:simple/random-host/random-bytes".to_string(),
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

        // Generate random integer in range
        interface.func_wrap_async(
            "random-range",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (min, max): (u64, u64)|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<u64, String>,)>> + Send> {
                let rng = Arc::clone(&rng2);
                let config = config2.clone();

                Box::new(async move {
                    // Record the random range call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/random-host/random-range".to_string(),
                        data: EventData::Random(RandomEventData::RandomRangeCall { min, max }),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(format!(
                            "Generating random number in range {} to {}",
                            min, max
                        )),
                    });

                    if min >= max {
                        let error_msg = format!("Invalid range: min ({}) >= max ({})", min, max);

                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/random-host/random-range".to_string(),
                            data: EventData::Random(RandomEventData::Error {
                                operation: "random-range".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error generating random range: {}",
                                error_msg
                            )),
                        });

                        return Ok((Err(error_msg),));
                    }

                    if max > config.max_int {
                        let error_msg = format!(
                            "Requested max value ({}) exceeds configured maximum ({})",
                            max, config.max_int
                        );

                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/random-host/random-range".to_string(),
                            data: EventData::Random(RandomEventData::Error {
                                operation: "random-range".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Error generating random range: {}",
                                error_msg
                            )),
                        });

                        return Ok((Err(error_msg),));
                    }

                    match rng.lock() {
                        Ok(mut generator) => {
                            let value = generator.gen_range(min..max);

                            // Record successful result
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/random-host/random-range".to_string(),
                                data: EventData::Random(RandomEventData::RandomRangeResult {
                                    min,
                                    max,
                                    value,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Successfully generated random number {} in range {} to {}",
                                    value, min, max
                                )),
                            });

                            Ok((Ok(value),))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to acquire RNG lock: {}", e);

                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/random-host/random-range".to_string(),
                                data: EventData::Random(RandomEventData::Error {
                                    operation: "random-range".to_string(),
                                    message: error_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error generating random range: {}",
                                    error_msg
                                )),
                            });

                            Ok((Err(error_msg),))
                        }
                    }
                })
            },
        )?;

        // Generate random float between 0.0 and 1.0
        interface.func_wrap_async(
            "random-float",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (): ()|
                  -> Box<dyn Future<Output = anyhow::Result<(Result<f64, String>,)>> + Send> {
                let rng = Arc::clone(&rng3);

                Box::new(async move {
                    // Record the random float call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/random-host/random-float".to_string(),
                        data: EventData::Random(RandomEventData::RandomFloatCall),
                        timestamp: chrono::Utc::now().timestamp_millis() as u64,
                        description: Some(
                            "Generating random float between 0.0 and 1.0".to_string(),
                        ),
                    });

                    match rng.lock() {
                        Ok(mut generator) => {
                            let value: f64 = generator.gen();

                            // Record successful result
                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/random-host/random-float".to_string(),
                                data: EventData::Random(RandomEventData::RandomFloatResult {
                                    value,
                                    success: true,
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Successfully generated random float: {}",
                                    value
                                )),
                            });

                            Ok((Ok(value),))
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to acquire RNG lock: {}", e);

                            ctx.data_mut().record_event(ChainEventData {
                                event_type: "theater:simple/random-host/random-float".to_string(),
                                data: EventData::Random(RandomEventData::Error {
                                    operation: "random-float".to_string(),
                                    message: error_msg.clone(),
                                }),
                                timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                description: Some(format!(
                                    "Error generating random float: {}",
                                    error_msg
                                )),
                            });

                            Ok((Err(error_msg),))
                        }
                    }
                })
            },
        )?;

        interface.func_wrap_async(
            "generate-uuid",
            move |mut ctx: StoreContextMut<'_, ActorStore>,
                  (): ()| -> Box<dyn Future<Output = anyhow::Result<(Result<String, String>,)>> + Send> {
                let rng = Arc::clone(&rng4);
                
                Box::new(async move {
                    // Record the UUID generation call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/random-host/generate-uuid".to_string(),
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
                                event_type: "theater:simple/random-host/generate-uuid".to_string(),
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
                                event_type: "theater:simple/random-host/generate-uuid".to_string(),
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

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "random-setup".to_string(),
            data: EventData::Random(RandomEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Random host functions setup completed successfully".to_string()),
        });

        info!("Random number generator host functions setup complete");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance,
    ) -> anyhow::Result<()> {
        // Random handler doesn't export functions to actors, only provides host functions
        Ok(())
    }

    fn name(&self) -> &str {
        "random"
    }

    fn imports(&self) -> Option<String> {
        Some("theater:simple/random".to_string())
    }

    fn exports(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theater::config::actor_manifest::RandomHandlerConfig;

    #[test]
    fn test_random_handler_config_defaults() {
        let config = RandomHandlerConfig {
            seed: None,
            max_bytes: 1024,
            max_int: 1000,
            allow_crypto_secure: false,
        };

        let handler = RandomHandler::new(config.clone(), None);
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

        let handler = RandomHandler::new(config, None);
        assert_eq!(handler.config.seed, Some(12345));
    }
}
