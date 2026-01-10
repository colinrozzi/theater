//! # Random Number Generator Handler
//!
//! Provides random number generation capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to generate random bytes, integers within ranges, and floating-point
//! numbers while maintaining security boundaries and resource limits.
//!
//! ## Architecture
//!
//! This handler manually implements WASI Random interfaces using `func_wrap` to ensure
//! proper recording of all host function calls for replay support.

pub mod events;

pub use events::RandomEventData;

use anyhow::Result;
use rand::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tracing::info;
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::ActorStore;
use theater::config::actor_manifest::RandomHandlerConfig;
use theater::config::permissions::RandomPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
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

    fn setup_random_interface(&self, actor_component: &mut ActorComponent) -> Result<()> {
        let mut interface = actor_component
            .linker
            .instance("wasi:random/random@0.2.3")
            .map_err(|e| anyhow::anyhow!("Could not get wasi:random/random interface: {}", e))?;

        let rng = Arc::clone(&self.rng);

        // get-random-bytes: func(len: u64) -> list<u8>
        let rng_clone = Arc::clone(&rng);
        interface
            .func_wrap(
                "get-random-bytes",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (len,): (u64,)| -> Result<(Vec<u8>,)> {
                    let len = len as usize;
                    let mut bytes = vec![0u8; len];

                    {
                        let mut generator = rng_clone.lock().unwrap();
                        generator.fill_bytes(&mut bytes);
                    }

                    // Record the call for replay
                    ctx.data_mut().record_host_function_call(
                        "wasi:random/random@0.2.3",
                        "get-random-bytes",
                        &len,
                        &bytes,
                    );

                    Ok((bytes,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap get-random-bytes: {}", e))?;

        // get-random-u64: func() -> u64
        let rng_clone = Arc::clone(&rng);
        interface
            .func_wrap(
                "get-random-u64",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    let value: u64 = {
                        let mut generator = rng_clone.lock().unwrap();
                        generator.gen()
                    };

                    // Record the call for replay
                    ctx.data_mut().record_host_function_call(
                        "wasi:random/random@0.2.3",
                        "get-random-u64",
                        &(),
                        &value,
                    );

                    Ok((value,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap get-random-u64: {}", e))?;

        info!("wasi:random/random@0.2.3 interface added");
        Ok(())
    }

    fn setup_insecure_interface(&self, actor_component: &mut ActorComponent) -> Result<()> {
        let mut interface = actor_component
            .linker
            .instance("wasi:random/insecure@0.2.3")
            .map_err(|e| anyhow::anyhow!("Could not get wasi:random/insecure interface: {}", e))?;

        let rng = Arc::clone(&self.rng);

        // get-insecure-random-bytes: func(len: u64) -> list<u8>
        let rng_clone = Arc::clone(&rng);
        interface
            .func_wrap(
                "get-insecure-random-bytes",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (len,): (u64,)| -> Result<(Vec<u8>,)> {
                    let len = len as usize;
                    let mut bytes = vec![0u8; len];

                    {
                        let mut generator = rng_clone.lock().unwrap();
                        generator.fill_bytes(&mut bytes);
                    }

                    // Record the call for replay
                    ctx.data_mut().record_host_function_call(
                        "wasi:random/insecure@0.2.3",
                        "get-insecure-random-bytes",
                        &len,
                        &bytes,
                    );

                    Ok((bytes,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap get-insecure-random-bytes: {}", e))?;

        // get-insecure-random-u64: func() -> u64
        let rng_clone = Arc::clone(&rng);
        interface
            .func_wrap(
                "get-insecure-random-u64",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    let value: u64 = {
                        let mut generator = rng_clone.lock().unwrap();
                        generator.gen()
                    };

                    // Record the call for replay
                    ctx.data_mut().record_host_function_call(
                        "wasi:random/insecure@0.2.3",
                        "get-insecure-random-u64",
                        &(),
                        &value,
                    );

                    Ok((value,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap get-insecure-random-u64: {}", e))?;

        info!("wasi:random/insecure@0.2.3 interface added");
        Ok(())
    }

    fn setup_insecure_seed_interface(&self, actor_component: &mut ActorComponent) -> Result<()> {
        let mut interface = actor_component
            .linker
            .instance("wasi:random/insecure-seed@0.2.3")
            .map_err(|e| anyhow::anyhow!("Could not get wasi:random/insecure-seed interface: {}", e))?;

        let rng = Arc::clone(&self.rng);

        // insecure-seed: func() -> tuple<u64, u64>
        interface
            .func_wrap(
                "insecure-seed",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<((u64, u64),)> {
                    let (seed1, seed2): (u64, u64) = {
                        let mut generator = rng.lock().unwrap();
                        (generator.gen(), generator.gen())
                    };

                    // Record the call for replay
                    ctx.data_mut().record_host_function_call(
                        "wasi:random/insecure-seed@0.2.3",
                        "insecure-seed",
                        &(),
                        &(seed1, seed2),
                    );

                    Ok(((seed1, seed2),))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap insecure-seed: {}", e))?;

        info!("wasi:random/insecure-seed@0.2.3 interface added");
        Ok(())
    }
}

impl Handler for RandomHandler
{
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(Self::new(self.config.clone(), self.permissions.clone()))
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
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
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        // Check if imports are already satisfied (e.g., by replay handler)
        let random_satisfied = ctx.is_satisfied("wasi:random/random@0.2.3");
        let insecure_satisfied = ctx.is_satisfied("wasi:random/insecure@0.2.3");
        let seed_satisfied = ctx.is_satisfied("wasi:random/insecure-seed@0.2.3");

        if random_satisfied && insecure_satisfied && seed_satisfied {
            info!("WASI random interfaces already satisfied (replay mode), skipping setup");
            return Ok(());
        }

        info!("Setting up WASI random host functions with manual func_wrap");

        // Setup wasi:random/random@0.2.3 interface (if not already satisfied)
        if !random_satisfied {
            self.setup_random_interface(actor_component)?;
            ctx.mark_satisfied("wasi:random/random@0.2.3");
        }

        // Setup wasi:random/insecure@0.2.3 interface (if not already satisfied)
        if !insecure_satisfied {
            self.setup_insecure_interface(actor_component)?;
            ctx.mark_satisfied("wasi:random/insecure@0.2.3");
        }

        // Setup wasi:random/insecure-seed@0.2.3 interface (if not already satisfied)
        if !seed_satisfied {
            self.setup_insecure_seed_interface(actor_component)?;
            ctx.mark_satisfied("wasi:random/insecure-seed@0.2.3");
        }

        info!("WASI random host functions setup complete");
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

    fn imports(&self) -> Option<Vec<String>> {
        // Handler provides WASI random interfaces (version 0.2.3)
        Some(vec![
            "wasi:random/random@0.2.3".to_string(),
            "wasi:random/insecure@0.2.3".to_string(),
            "wasi:random/insecure-seed@0.2.3".to_string(),
        ])
    }

    fn exports(&self) -> Option<Vec<String>> {
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
