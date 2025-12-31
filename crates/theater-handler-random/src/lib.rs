//! # Random Number Generator Handler
//!
//! Provides random number generation capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to generate random bytes, integers within ranges, and floating-point
//! numbers while maintaining security boundaries and resource limits.
//!
//! ## Architecture
//!
//! This handler uses wasmtime's bindgen to generate type-safe Host traits from
//! the WASI Random WIT definitions. This ensures compile-time verification that
//! our implementation matches the WASI specification.

pub mod events;
pub mod bindings;
pub mod host_impl;

pub use events::RandomEventData;
pub use host_impl::set_thread_rng;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tracing::info;

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::RandomHandlerConfig;
use theater::config::permissions::RandomPermissions;
use theater::events::EventPayload;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

use crate::events::RandomEventData as HandlerEventData;

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

    // Note: The old manual setup_wasi_random method has been replaced by
    // bindgen-generated add_to_linker calls in setup_host_functions.
    // The Host trait implementations are in host_impl.rs.
}

impl<E> Handler<E> for RandomHandler
where
    E: EventPayload + Clone + From<HandlerEventData>,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(Self::new(self.config.clone(), self.permissions.clone()))
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
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
        actor_component: &mut ActorComponent<E>,
        _ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        use crate::bindings;
        use theater::actor::ActorStore;

        info!("Setting up WASI random host functions using bindgen-generated add_to_linker");

        // Set up the thread-local RNG for this actor
        set_thread_rng(Arc::clone(&self.rng));

        // Add wasi:random/random interface
        bindings::wasi::random::random::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        info!("wasi:random/random interface added");

        // Add wasi:random/insecure interface
        bindings::wasi::random::insecure::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        info!("wasi:random/insecure interface added");

        // Add wasi:random/insecure-seed interface
        bindings::wasi::random::insecure_seed::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        info!("wasi:random/insecure-seed interface added");

        info!("WASI random host functions setup complete (using bindgen traits)");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance<E>,
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
