//! Host trait implementations for WASI Random interfaces
//!
//! These implementations provide random number generation to actors while
//! recording all operations in the event chain for replay.

use crate::bindings::{RandomHost, InsecureHost, InsecureSeedHost};
use crate::events::RandomEventData;
use anyhow::Result;
use rand::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::sync::{Arc, Mutex};
use theater::actor::ActorStore;
use tracing::debug;

/// Thread-local RNG for random operations
/// This is initialized per-actor when the handler is set up
thread_local! {
    static THREAD_RNG: std::cell::RefCell<Option<Arc<Mutex<ChaCha20Rng>>>> = const { std::cell::RefCell::new(None) };
}

/// Set the RNG for the current thread/actor
pub fn set_thread_rng(rng: Arc<Mutex<ChaCha20Rng>>) {
    THREAD_RNG.with(|r| {
        *r.borrow_mut() = Some(rng);
    });
}

/// Get the RNG for the current thread/actor
fn get_rng() -> Arc<Mutex<ChaCha20Rng>> {
    THREAD_RNG.with(|r| {
        r.borrow()
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(ChaCha20Rng::from_entropy())))
    })
}

// Implement RandomHost for ActorStore
impl RandomHost for ActorStore
{
    async fn get_random_bytes(&mut self, len: u64) -> Result<Vec<u8>> {
        let len = len as usize;
        debug!("wasi:random/random get-random-bytes: requesting {} bytes", len);

        self.record_handler_event(
            "wasi:random/random/get-random-bytes".to_string(),
            RandomEventData::RandomBytesCall { requested_size: len },
            Some(format!("WASI random: requesting {} bytes", len)),
        );

        let rng = get_rng();
        let mut bytes = vec![0u8; len];

        let result = {
            match rng.lock() {
                Ok(mut generator) => {
                    generator.fill_bytes(&mut bytes);
                    Ok(bytes.clone())
                }
                Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e)),
            }
        };

        match &result {
            Ok(_) => {
                self.record_handler_event(
                    "wasi:random/random/get-random-bytes".to_string(),
                    RandomEventData::RandomBytesResult {
                        generated_size: len,
                        bytes: Some(bytes),
                        success: true,
                    },
                    Some(format!("WASI random: generated {} bytes", len)),
                );
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:random/random/get-random-bytes".to_string(),
                    RandomEventData::Error {
                        operation: "get-random-bytes".to_string(),
                        message: e.to_string(),
                    },
                    Some(format!("WASI random error: {}", e)),
                );
            }
        }

        result
    }

    async fn get_random_u64(&mut self) -> Result<u64> {
        debug!("wasi:random/random get-random-u64");

        self.record_handler_event(
            "wasi:random/random/get-random-u64".to_string(),
            RandomEventData::RandomU64Call,
            Some("WASI random: requesting random u64".to_string()),
        );

        let rng = get_rng();

        let result = {
            match rng.lock() {
                Ok(mut generator) => {
                    let value: u64 = generator.gen();
                    Ok(value)
                }
                Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e)),
            }
        };

        match &result {
            Ok(value) => {
                self.record_handler_event(
                    "wasi:random/random/get-random-u64".to_string(),
                    RandomEventData::RandomU64Result {
                        value: *value,
                        success: true,
                    },
                    Some(format!("WASI random: generated u64 = {}", value)),
                );
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:random/random/get-random-u64".to_string(),
                    RandomEventData::Error {
                        operation: "get-random-u64".to_string(),
                        message: e.to_string(),
                    },
                    Some(format!("WASI random error: {}", e)),
                );
            }
        }

        result
    }
}

// Implement InsecureHost for ActorStore
impl InsecureHost for ActorStore
{
    async fn get_insecure_random_bytes(&mut self, len: u64) -> Result<Vec<u8>> {
        let len = len as usize;
        debug!("wasi:random/insecure get-insecure-random-bytes: requesting {} bytes", len);

        self.record_handler_event(
            "wasi:random/insecure/get-insecure-random-bytes".to_string(),
            RandomEventData::InsecureRandomBytesCall { requested_size: len },
            Some(format!("WASI insecure random: requesting {} bytes", len)),
        );

        let rng = get_rng();
        let mut bytes = vec![0u8; len];

        let result = {
            match rng.lock() {
                Ok(mut generator) => {
                    generator.fill_bytes(&mut bytes);
                    Ok(bytes.clone())
                }
                Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e)),
            }
        };

        match &result {
            Ok(_) => {
                self.record_handler_event(
                    "wasi:random/insecure/get-insecure-random-bytes".to_string(),
                    RandomEventData::InsecureRandomBytesResult {
                        generated_size: len,
                        bytes: Some(bytes),
                        success: true,
                    },
                    Some(format!("WASI insecure random: generated {} bytes", len)),
                );
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:random/insecure/get-insecure-random-bytes".to_string(),
                    RandomEventData::Error {
                        operation: "get-insecure-random-bytes".to_string(),
                        message: e.to_string(),
                    },
                    Some(format!("WASI insecure random error: {}", e)),
                );
            }
        }

        result
    }

    async fn get_insecure_random_u64(&mut self) -> Result<u64> {
        debug!("wasi:random/insecure get-insecure-random-u64");

        self.record_handler_event(
            "wasi:random/insecure/get-insecure-random-u64".to_string(),
            RandomEventData::InsecureRandomU64Call,
            Some("WASI insecure random: requesting random u64".to_string()),
        );

        let rng = get_rng();

        let result = {
            match rng.lock() {
                Ok(mut generator) => {
                    let value: u64 = generator.gen();
                    Ok(value)
                }
                Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e)),
            }
        };

        match &result {
            Ok(value) => {
                self.record_handler_event(
                    "wasi:random/insecure/get-insecure-random-u64".to_string(),
                    RandomEventData::InsecureRandomU64Result {
                        value: *value,
                        success: true,
                    },
                    Some(format!("WASI insecure random: generated u64 = {}", value)),
                );
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:random/insecure/get-insecure-random-u64".to_string(),
                    RandomEventData::Error {
                        operation: "get-insecure-random-u64".to_string(),
                        message: e.to_string(),
                    },
                    Some(format!("WASI insecure random error: {}", e)),
                );
            }
        }

        result
    }
}

// Implement InsecureSeedHost for ActorStore
impl InsecureSeedHost for ActorStore
{
    async fn insecure_seed(&mut self) -> Result<(u64, u64)> {
        debug!("wasi:random/insecure-seed insecure-seed");

        self.record_handler_event(
            "wasi:random/insecure-seed/insecure-seed".to_string(),
            RandomEventData::InsecureSeedCall,
            Some("WASI insecure-seed: requesting seed".to_string()),
        );

        let rng = get_rng();

        let result = {
            match rng.lock() {
                Ok(mut generator) => {
                    let seed1: u64 = generator.gen();
                    let seed2: u64 = generator.gen();
                    Ok((seed1, seed2))
                }
                Err(e) => Err(anyhow::anyhow!("RNG lock failed: {}", e)),
            }
        };

        match &result {
            Ok((seed1, seed2)) => {
                self.record_handler_event(
                    "wasi:random/insecure-seed/insecure-seed".to_string(),
                    RandomEventData::InsecureSeedResult {
                        seed: (*seed1, *seed2),
                        success: true,
                    },
                    Some(format!("WASI insecure-seed: generated seed ({}, {})", seed1, seed2)),
                );
            }
            Err(e) => {
                self.record_handler_event(
                    "wasi:random/insecure-seed/insecure-seed".to_string(),
                    RandomEventData::Error {
                        operation: "insecure-seed".to_string(),
                        message: e.to_string(),
                    },
                    Some(format!("WASI insecure-seed error: {}", e)),
                );
            }
        }

        result
    }
}
