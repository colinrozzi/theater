//! Cryptographically-secure randomness handler for Theater actors.
//!
//! A wasm guest has no ambient entropy source, so secure randomness must come
//! from the host. This handler exposes the OS CSPRNG as `theater:simple/random`
//! (see `random.pact`). It is deliberately a RAW primitive: secure bytes, a
//! u64, a bounded range, and a v4 uuid — nothing built on top.
//!
//! DETERMINISTIC REPLAY: these are ordinary host-function calls, so the Pack
//! `CallInterceptor` records each call's output to the actor's chain
//! automatically. A first run draws fresh OS entropy; a replay returns the
//! recorded bytes from the chain. Randomness fits chain=truth with no seed.
//!
//! The handler is STATELESS (no sockets, no background tasks, no callbacks), so
//! `setup` simply waits for shutdown and returns.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use theater::actor::handle::ActorHandle;
use theater::config::actor_manifest::HandlerConfig;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::pack_bridge::{
    pact_result_host_fn, parse_pact, plain_host_fn, InterfaceImpl, TypeHash, Value, ValueType,
};
use theater::shutdown::ShutdownReceiver;

const RANDOM_PACT: &str = include_str!("../random.pact");

/// Default cap on a single `random-bytes` call (1 MiB) — guards against an
/// accidental or hostile huge allocation.
fn default_max_bytes() -> u32 {
    1024 * 1024
}

/// Configuration + limits for the random handler (handler-owned, the current
/// per-handler-config idiom — cf. the tcp/udp handlers).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RandomHandlerConfig {
    /// Max bytes returnable from one `random-bytes` call. Default 1 MiB.
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u32,
}

impl Default for RandomHandlerConfig {
    fn default() -> Self {
        Self {
            max_bytes: default_max_bytes(),
        }
    }
}

fn random_interface() -> InterfaceImpl {
    let pact = parse_pact(RANDOM_PACT).expect("embedded random.pact should be valid");
    InterfaceImpl::from_pact(&pact)
}

/// Handler providing cryptographically-secure randomness to WebAssembly actors.
#[derive(Clone)]
pub struct RandomHandler {
    config: RandomHandlerConfig,
}

impl RandomHandler {
    pub fn new(config: RandomHandlerConfig) -> Self {
        Self { config }
    }

    pub fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![random_interface()]
    }
}

// ── parsing helpers (unwrap the single-arg tuple the ABI may wrap in) ────────

fn as_u32(input: &Value) -> Result<u32, Value> {
    match input {
        Value::U32(n) => Ok(*n),
        Value::U64(n) => Ok(*n as u32),
        Value::Tuple(f) if f.len() == 1 => as_u32(&f[0]),
        _ => Err(Value::String("expected u32".to_string())),
    }
}

fn two_u64(input: &Value) -> Result<(u64, u64), Value> {
    match input {
        Value::Tuple(f) if f.len() == 2 => {
            let a = as_u64(&f[0])?;
            let b = as_u64(&f[1])?;
            Ok((a, b))
        }
        _ => Err(Value::String("expected (u64, u64)".to_string())),
    }
}

fn as_u64(v: &Value) -> Result<u64, Value> {
    match v {
        Value::U64(n) => Ok(*n),
        Value::U32(n) => Ok(*n as u64),
        _ => Err(Value::String("expected u64".to_string())),
    }
}

/// Fill a buffer from the OS CSPRNG, mapping failure to a pact Err value.
fn fill_secure(buf: &mut [u8]) -> Result<(), Value> {
    getrandom::getrandom(buf).map_err(|e| Value::String(format!("OS entropy source failed: {e}")))
}

/// A u64 straight from OS entropy. getrandom(2)/urandom does not fail post-boot
/// on the deploy target; an entropy failure is unrecoverable, so (like the uuid
/// crate's own v4) we treat it as fatal rather than silently returning a
/// non-random value.
fn secure_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom::getrandom(&mut b).expect("OS CSPRNG (getrandom) must be available");
    u64::from_le_bytes(b)
}

/// Uniform in [min, max) via rejection sampling — unbiased, unlike `% span`.
fn uniform_in(min: u64, max: u64) -> u64 {
    let span = max - min; // caller guarantees max > min
                          // Reject the top partial bucket so every value in [0, span) is equally likely.
    let zone = u64::MAX - (u64::MAX % span);
    loop {
        let r = secure_u64();
        if r < zone {
            return min + (r % span);
        }
    }
}

fn bytes_value(buf: Vec<u8>) -> Value {
    Value::List {
        elem_type: ValueType::U8,
        items: buf.into_iter().map(Value::U8).collect(),
    }
}

impl Handler for RandomHandler {
    fn create_instance(&self, config: Option<&HandlerConfig>) -> Box<dyn Handler> {
        let cfg = match config {
            Some(c) => c.parse::<RandomHandlerConfig>().unwrap_or_else(|e| {
                warn!("invalid random handler config: {}; using default", e);
                self.config.clone()
            }),
            None => self.config.clone(),
        };
        Box::new(RandomHandler { config: cfg })
    }

    fn name(&self) -> &str {
        "random"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(
            self.interfaces()
                .iter()
                .map(|i| i.name().to_string())
                .collect(),
        )
    }

    fn exports(&self) -> Option<Vec<String>> {
        None // no callback interface — randomness is pull-only
    }

    fn interface_hashes(&self) -> Vec<(String, TypeHash)> {
        self.interfaces()
            .iter()
            .map(|i| (i.name().to_string(), i.hash()))
            .collect()
    }

    fn interfaces(&self) -> Vec<InterfaceImpl> {
        vec![random_interface()]
    }

    fn setup(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
        _event_rx: theater::handler::HandlerEventReceiver,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
        info!("Random handler setup");
        Box::pin(async move {
            // Nothing to hold or release — just park until shutdown.
            let _shutdown_signal = shutdown_receiver.wait_for_shutdown().await;
            info!("Random handler received shutdown");
            Ok(())
        })
    }

    fn register_host_functions(
        &mut self,
        imports: &mut theater::pack_bridge::HostImports,
        ctx: &mut HandlerContext,
    ) -> anyhow::Result<()> {
        info!("Setting up random host functions");
        if ctx.is_satisfied("theater:simple/random") {
            return Ok(());
        }

        // ---- random-bytes ----
        {
            let max_bytes = self.config.max_bytes;
            imports.define(
                "theater:simple/random",
                "random-bytes",
                pact_result_host_fn(move |input: Value| async move {
                    let len = as_u32(&input)?;
                    if len > max_bytes {
                        return Err(Value::String(format!(
                            "random-bytes: len {len} exceeds max_bytes {max_bytes}"
                        )));
                    }
                    let mut buf = vec![0u8; len as usize];
                    fill_secure(&mut buf)?;
                    Ok::<Value, Value>(bytes_value(buf))
                }),
            );
        }

        // ---- random-u64 ----
        imports.define(
            "theater:simple/random",
            "random-u64",
            plain_host_fn(move |_input: Value| async move { Value::U64(secure_u64()) }),
        );

        // ---- random-range ----
        imports.define(
            "theater:simple/random",
            "random-range",
            pact_result_host_fn(move |input: Value| async move {
                let (min, max) = two_u64(&input)?;
                if min >= max {
                    return Err(Value::String(format!(
                        "random-range: min ({min}) must be < max ({max})"
                    )));
                }
                Ok::<Value, Value>(Value::U64(uniform_in(min, max)))
            }),
        );

        // ---- uuid ----
        imports.define(
            "theater:simple/random",
            "uuid",
            plain_host_fn(move |_input: Value| async move {
                Value::String(uuid::Uuid::new_v4().to_string())
            }),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests;
