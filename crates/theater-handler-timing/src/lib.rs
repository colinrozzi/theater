//! # Timing Handler
//!
//! Provides timing capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to get the current time, sleep for durations,
//! and wait until specific deadlines.
//!
//! ## Architecture
//!
//! This handler uses wasmtime's bindgen to generate type-safe Host traits from
//! the WASI Clocks WIT definitions. This ensures compile-time verification that
//! our implementation matches the WASI specification.

pub mod events;
pub mod bindings;
pub mod host_impl;

use anyhow::Result;
use chrono::Utc;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tokio::time::{sleep, Duration};
use tracing::{info, error};
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::types::ActorError;
use theater::config::actor_manifest::TimingHostConfig;
use theater::config::enforcement::PermissionChecker;
use theater::config::permissions::TimingPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

pub use events::TimingEventData;

/// Represents a pollable resource for timing operations
///
/// Pollables are used in the Component Model to represent async events.
/// For timing, they represent events that become "ready" when a specific
/// time is reached or a duration has elapsed.
#[derive(Debug, Clone)]
pub struct Pollable {
    /// The monotonic clock instant (in nanoseconds) when this pollable becomes ready
    pub deadline: u64,

    /// The kind of pollable (instant or duration-based)
    pub kind: PollableKind,
}

/// The kind of timing pollable
#[derive(Debug, Clone)]
pub enum PollableKind {
    /// Becomes ready at a specific monotonic instant
    MonotonicInstant(u64),

    /// Becomes ready after a duration from creation time
    MonotonicDuration { duration: u64, created_at: u64 },
}

#[derive(Clone)]
pub struct TimingHandler {
    #[allow(dead_code)]
    config: TimingHostConfig,
    permissions: Option<TimingPermissions>,
}

#[derive(Error, Debug)]
pub enum TimingError {
    #[error("Timing error: {0}")]
    TimingError(String),

    #[error("Duration too long: {duration} ms exceeds maximum of {max} ms")]
    DurationTooLong { duration: u64, max: u64 },

    #[error("Duration too short: {duration} ms is below minimum of {min} ms")]
    DurationTooShort { duration: u64, min: u64 },

    #[error("Invalid deadline: {timestamp} is in the past")]
    InvalidDeadline { timestamp: u64 },

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),
}

impl TimingHandler {
    pub fn new(config: TimingHostConfig, permissions: Option<TimingPermissions>) -> Self {
        Self {
            config,
            permissions,
        }
    }

    // Note: The old manual setup_wasi_wall_clock, setup_wasi_monotonic_clock, and setup_wasi_poll
    // methods have been replaced by bindgen-generated add_to_linker calls in setup_host_functions_impl.
    // The Host trait implementations are in host_impl.rs.

    fn setup_host_functions_impl(&mut self, actor_component: &mut ActorComponent) -> Result<()> {

        info!("Setting up timing host functions");

        let mut interface = match actor_component.linker.instance("theater:simple/timing") {
            Ok(interface) => {                interface
            }
            Err(e) => {                return Err(anyhow::anyhow!(
                    "Could not instantiate theater:simple/timing: {}",
                    e
                ));
            }
        };

        let permissions = self.permissions.clone();

        // Implementation of the now() function
        let _ = interface
            .func_wrap(
                "now",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    let now = Utc::now().timestamp_millis() as u64;

                    // Record with standardized HostFunctionCall format for replay
                    ctx.data_mut().record_host_function_call(
                        "theater:simple/timing",
                        "now",
                        &(),   // no input
                        &now,  // output: timestamp
                    );

                    Ok((now,))
                },
            )
            .map_err(|e| {                anyhow::anyhow!("Failed to wrap now function: {}", e)
            })?;

        // Implementation of the sleep() function
        let permissions_clone = permissions.clone();
        let _ = interface
            .func_wrap_async(
                "sleep",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (duration,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {

                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone,
                        "sleep",
                        duration,
                    ) {
                        let result: Result<(), String> = Err(format!("Permission denied: {}", e));

                        // Record the call with error result for replay
                        ctx.data_mut().record_host_function_call(
                            "theater:simple/timing",
                            "sleep",
                            &duration,
                            &result,
                        );

                        return Box::new(futures::future::ready(Ok((result,))));
                    }

                    let duration_clone = duration;

                    Box::new(async move {
                        if duration_clone > 0 {
                            sleep(Duration::from_millis(duration_clone)).await;
                        }

                        let result: Result<(), String> = Ok(());

                        // Record with standardized HostFunctionCall format for replay
                        ctx.data_mut().record_host_function_call(
                            "theater:simple/timing",
                            "sleep",
                            &duration_clone,
                            &result,
                        );

                        Ok((result,))
                    })
                },
            )
            .map_err(|e| {                anyhow::anyhow!("Failed to wrap sleep function: {}", e)
            })?;

        // Implementation of the deadline() function
        let permissions_clone2 = permissions.clone();
        let _ = interface
            .func_wrap_async(
                "deadline",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (timestamp,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let now = Utc::now().timestamp_millis() as u64;

                    if timestamp <= now {
                        let result: Result<(), String> = Ok(());

                        // Record immediately - deadline already passed
                        ctx.data_mut().record_host_function_call(
                            "theater:simple/timing",
                            "deadline",
                            &timestamp,
                            &result,
                        );

                        return Box::new(futures::future::ready(Ok((result,))));
                    }

                    let duration = timestamp - now;

                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone2,
                        "deadline",
                        duration,
                    ) {
                        let result: Result<(), String> = Err(format!("Permission denied: {}", e));

                        ctx.data_mut().record_host_function_call(
                            "theater:simple/timing",
                            "deadline",
                            &timestamp,
                            &result,
                        );

                        return Box::new(futures::future::ready(Ok((result,))));
                    }

                    let timestamp_clone = timestamp;

                    Box::new(async move {
                        sleep(Duration::from_millis(duration)).await;

                        let result: Result<(), String> = Ok(());

                        // Record with standardized HostFunctionCall format for replay
                        ctx.data_mut().record_host_function_call(
                            "theater:simple/timing",
                            "deadline",
                            &timestamp_clone,
                            &result,
                        );

                        Ok((result,))
                    })
                },
            )
            .map_err(|e| {                anyhow::anyhow!("Failed to wrap deadline function: {}", e)
            })?;
        info!("Theater timing host functions added successfully");

        // Setup WASI-compliant clock and poll interfaces using bindgen-generated add_to_linker
        info!("Setting up WASI clocks/poll interfaces using bindgen");

        use crate::bindings;
        use theater::actor::ActorStore;

        // Add wasi:clocks/wall-clock interface
        bindings::wasi::clocks::wall_clock::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        info!("wasi:clocks/wall-clock interface added");

        // Add wasi:clocks/monotonic-clock interface
        bindings::wasi::clocks::monotonic_clock::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        info!("wasi:clocks/monotonic-clock interface added");

        // Add wasi:io/poll interface
        bindings::wasi::io::poll::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore| state,
        )?;
        info!("wasi:io/poll interface added");

        info!("WASI clock/poll interfaces setup complete (using bindgen traits)");

        Ok(())
    }

    fn add_export_functions_impl(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No export functions needed for timing handler");
        Ok(())
    }

    async fn start_impl(
        &self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting timing handler");
        Ok(())
    }
}

impl Handler for TimingHandler
{
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let handler = self.clone();
        Box::pin(async move { handler.start_impl(actor_handle, shutdown_receiver).await })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
        _ctx: &mut HandlerContext,
    ) -> Result<()> {
        self.setup_host_functions_impl(actor_component)
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> Result<()> {
        self.add_export_functions_impl(actor_instance)
    }

    fn name(&self) -> &str {
        "timing"
    }

    fn imports(&self) -> Option<Vec<String>> {
        // Handler provides WASI clocks and poll interfaces (version 0.2.3)
        // Supports: wall-clock, monotonic-clock, and poll
        Some(vec![
            "wasi:clocks/wall-clock@0.2.3".to_string(),
            "wasi:clocks/monotonic-clock@0.2.3".to_string(),
            "wasi:io/poll@0.2.3".to_string(),
        ])
    }

    fn exports(&self) -> Option<Vec<String>> {
        None
    }
}
