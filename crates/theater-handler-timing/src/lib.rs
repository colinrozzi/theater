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
use theater::events::EventPayload;
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

    fn setup_host_functions_impl<E>(&mut self, actor_component: &mut ActorComponent<E>) -> Result<()>
    where
        E: EventPayload + Clone + From<TimingEventData>,
    {
        // Record setup start
        actor_component.actor_store.record_handler_event(
            "timing-setup".to_string(),
            TimingEventData::HandlerSetupStart,
            Some("Starting timing host function setup".to_string()),
        );

        info!("Setting up timing host functions");

        let mut interface = match actor_component.linker.instance("theater:simple/timing") {
            Ok(interface) => {
                actor_component.actor_store.record_handler_event(
                    "timing-setup".to_string(),
                    TimingEventData::LinkerInstanceSuccess,
                    Some("Successfully created linker instance".to_string()),
                );
                interface
            }
            Err(e) => {
                actor_component.actor_store.record_handler_event(
                    "timing-setup".to_string(),
                    TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    },
                    Some(format!("Failed to create linker instance: {}", e)),
                );
                return Err(anyhow::anyhow!(
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
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>, ()| -> Result<(u64,)> {
                    let now = Utc::now().timestamp_millis() as u64;

                    ctx.data_mut().record_handler_event(
                        "theater:simple/timing/now".to_string(),
                        TimingEventData::NowCall {},
                        Some("Getting current timestamp".to_string()),
                    );

                    ctx.data_mut().record_handler_event(
                        "theater:simple/timing/now".to_string(),
                        TimingEventData::NowResult { timestamp: now },
                        Some(format!("Current timestamp: {}", now)),
                    );

                    Ok((now,))
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_handler_event(
                    "timing-setup".to_string(),
                    TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "now_function_wrap".to_string(),
                    },
                    Some(format!("Failed to wrap now function: {}", e)),
                );
                anyhow::anyhow!("Failed to wrap now function: {}", e)
            })?;

        // Implementation of the sleep() function
        let permissions_clone = permissions.clone();
        let _ = interface
            .func_wrap_async(
                "sleep",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (duration,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let _now = Utc::now().timestamp_millis() as u64;

                    ctx.data_mut().record_handler_event(
                        "theater:simple/timing/sleep".to_string(),
                        TimingEventData::SleepCall { duration },
                        Some(format!("Sleeping for {} ms", duration)),
                    );

                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone,
                        "sleep",
                        duration,
                    ) {
                        ctx.data_mut().record_handler_event(
                            "theater:simple/timing/permission-denied".to_string(),
                            TimingEventData::PermissionDenied {
                                operation: "sleep".to_string(),
                                reason: e.to_string(),
                            },
                            Some(format!("Permission denied for sleep operation: {}", e)),
                        );

                        return Box::new(futures::future::ready(Ok((Err(format!("Permission denied: {}", e)),))));
                    }

                    let duration_clone = duration;

                    Box::new(async move {
                        if duration_clone > 0 {
                            sleep(Duration::from_millis(duration_clone)).await;
                        }

                        let _end_time = Utc::now().timestamp_millis() as u64;

                        ctx.data_mut().record_handler_event(
                            "theater:simple/timing/sleep".to_string(),
                            TimingEventData::SleepResult {
                                duration: duration_clone,
                                success: true,
                            },
                            Some(format!("Successfully slept for {} ms", duration_clone)),
                        );

                        Ok((Ok(()),))
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_handler_event(
                    "timing-setup".to_string(),
                    TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "sleep_function_wrap".to_string(),
                    },
                    Some(format!("Failed to wrap sleep function: {}", e)),
                );
                anyhow::anyhow!("Failed to wrap sleep function: {}", e)
            })?;

        // Implementation of the deadline() function
        let permissions_clone2 = permissions.clone();
        let _ = interface
            .func_wrap_async(
                "deadline",
                move |mut ctx: StoreContextMut<'_, ActorStore<E>>,
                      (timestamp,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let now = Utc::now().timestamp_millis() as u64;

                    ctx.data_mut().record_handler_event(
                        "theater:simple/timing/deadline".to_string(),
                        TimingEventData::DeadlineCall { timestamp },
                        Some(format!("Waiting until timestamp: {}", timestamp)),
                    );

                    if timestamp <= now {
                        let success_msg = "Deadline already passed, continuing immediately";

                        ctx.data_mut().record_handler_event(
                            "theater:simple/timing/deadline".to_string(),
                            TimingEventData::DeadlineResult {
                                timestamp,
                                success: true,
                            },
                            Some(success_msg.to_string()),
                        );

                        return Box::new(futures::future::ready(Ok((Ok(()),))));
                    }

                    let duration = timestamp - now;

                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone2,
                        "deadline",
                        duration,
                    ) {
                        ctx.data_mut().record_handler_event(
                            "theater:simple/timing/permission-denied".to_string(),
                            TimingEventData::PermissionDenied {
                                operation: "deadline".to_string(),
                                reason: e.to_string(),
                            },
                            Some(format!("Permission denied for deadline operation: {}", e)),
                        );

                        return Box::new(futures::future::ready(Ok((Err(format!("Permission denied: {}", e)),))));
                    }

                    let timestamp_clone = timestamp;

                    Box::new(async move {
                        sleep(Duration::from_millis(duration)).await;

                        let end_time = Utc::now().timestamp_millis() as u64;
                        let reached_deadline = end_time >= timestamp_clone;

                        ctx.data_mut().record_handler_event(
                            "theater:simple/timing/deadline".to_string(),
                            TimingEventData::DeadlineResult {
                                timestamp: timestamp_clone,
                                success: reached_deadline,
                            },
                            Some(format!(
                                "Deadline wait completed at {}. Target was {}",
                                end_time,
                                timestamp_clone
                            )),
                        );

                        Ok((Ok(()),))
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_handler_event(
                    "timing-setup".to_string(),
                    TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "deadline_function_wrap".to_string(),
                    },
                    Some(format!("Failed to wrap deadline function: {}", e)),
                );
                anyhow::anyhow!("Failed to wrap deadline function: {}", e)
            })?;

        actor_component.actor_store.record_handler_event(
            "timing-setup".to_string(),
            TimingEventData::HandlerSetupSuccess,
            Some("Timing host functions setup completed successfully".to_string()),
        );

        info!("Theater timing host functions added successfully");

        // Setup WASI-compliant clock and poll interfaces using bindgen-generated add_to_linker
        info!("Setting up WASI clocks/poll interfaces using bindgen");

        use crate::bindings;
        use theater::actor::ActorStore;

        // Add wasi:clocks/wall-clock interface
        bindings::wasi::clocks::wall_clock::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        info!("wasi:clocks/wall-clock interface added");

        // Add wasi:clocks/monotonic-clock interface
        bindings::wasi::clocks::monotonic_clock::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        info!("wasi:clocks/monotonic-clock interface added");

        // Add wasi:io/poll interface
        bindings::wasi::io::poll::add_to_linker(
            &mut actor_component.linker,
            |state: &mut ActorStore<E>| state,
        )?;
        info!("wasi:io/poll interface added");

        info!("WASI clock/poll interfaces setup complete (using bindgen traits)");

        Ok(())
    }

    fn add_export_functions_impl<E>(&self, _actor_instance: &mut ActorInstance<E>) -> Result<()>
    where
        E: EventPayload + Clone + From<TimingEventData>,
    {
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

impl<E> Handler<E> for TimingHandler
where
    E: EventPayload + Clone + From<TimingEventData>,
{
    fn create_instance(&self) -> Box<dyn Handler<E>> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance<E>,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let handler = self.clone();
        Box::pin(async move { handler.start_impl(actor_handle, shutdown_receiver).await })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent<E>,
        _ctx: &mut HandlerContext,
    ) -> Result<()> {
        self.setup_host_functions_impl(actor_component)
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance<E>,
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
