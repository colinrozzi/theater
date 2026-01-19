//! # Timing Handler
//!
//! Provides WASI-compliant timing capabilities to WebAssembly actors in the Theater system.
//! This handler implements the WASI clocks and poll interfaces for time-related operations.
//!
//! ## Interfaces Provided
//!
//! - `wasi:clocks/wall-clock@0.2.3` - Wall clock time (real-world time)
//! - `wasi:clocks/monotonic-clock@0.2.3` - Monotonic clock for measuring durations
//! - `wasi:io/poll@0.2.3` - Polling interface for async operations
//!
//! ## Architecture
//!
//! This handler manually implements WASI interfaces using `func_wrap` to ensure
//! proper recording of all host function calls for replay support.

pub mod events;

use anyhow::Result;
use chrono::Utc;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tracing::{debug, info};
use val_serde::IntoSerializableVal;
use wasmtime::component::{Resource, ResourceType};
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::types::ActorError;
use theater::actor::ActorStore;
use theater::config::actor_manifest::TimingHostConfig;
use theater::config::permissions::TimingPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

pub use events::TimingEventData;

/// Represents a pollable resource for timing operations
#[derive(Debug, Clone)]
pub struct Pollable {
    /// The monotonic clock instant (in nanoseconds) when this pollable becomes ready
    pub deadline: u64,
}

#[derive(Clone)]
pub struct TimingHandler {
    #[allow(dead_code)]
    config: TimingHostConfig,
    #[allow(dead_code)]
    permissions: Option<TimingPermissions>,
}

#[derive(Error, Debug)]
pub enum TimingError {
    #[error("Timing error: {0}")]
    TimingError(String),

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

    fn setup_wall_clock(&self, actor_component: &mut ActorComponent) -> Result<()> {
        let mut interface = match actor_component
            .linker
            .instance("wasi:clocks/wall-clock@0.2.3")
        {
            Ok(i) => i,
            Err(_) => {
                debug!("wasi:clocks/wall-clock@0.2.3 not imported by component, skipping");
                return Ok(());
            }
        };

        // now: func() -> datetime
        interface
            .func_wrap(
                "now",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<((u64, u32),)> {
                    debug!("wasi:clocks/wall-clock now");
                    let now = Utc::now();
                    let seconds = now.timestamp() as u64;
                    let nanoseconds = now.timestamp_subsec_nanos();

                    ctx.data_mut().record_host_function_call(
                        "wasi:clocks/wall-clock@0.2.3",
                        "now",
                        ().into_serializable_val(),
                        val_serde::SerializableVal::Tuple(vec![
                            seconds.into_serializable_val(),
                            nanoseconds.into_serializable_val(),
                        ]),
                    );

                    Ok(((seconds, nanoseconds),))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap wall-clock now: {}", e))?;

        // resolution: func() -> datetime
        interface
            .func_wrap(
                "resolution",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<((u64, u32),)> {
                    debug!("wasi:clocks/wall-clock resolution");
                    let seconds = 0u64;
                    let nanoseconds = 1u32;

                    ctx.data_mut().record_host_function_call(
                        "wasi:clocks/wall-clock@0.2.3",
                        "resolution",
                        ().into_serializable_val(),
                        val_serde::SerializableVal::Tuple(vec![
                            seconds.into_serializable_val(),
                            nanoseconds.into_serializable_val(),
                        ]),
                    );

                    Ok(((seconds, nanoseconds),))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap wall-clock resolution: {}", e))?;

        info!("wasi:clocks/wall-clock@0.2.3 interface added");
        Ok(())
    }

    fn setup_monotonic_clock(&self, actor_component: &mut ActorComponent) -> Result<()> {
        let mut interface = match actor_component
            .linker
            .instance("wasi:clocks/monotonic-clock@0.2.3")
        {
            Ok(i) => i,
            Err(_) => {
                debug!("wasi:clocks/monotonic-clock@0.2.3 not imported by component, skipping");
                return Ok(());
            }
        };

        // now: func() -> instant (u64 nanoseconds)
        interface
            .func_wrap(
                "now",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    debug!("wasi:clocks/monotonic-clock now");
                    let now = Utc::now();
                    let instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

                    ctx.data_mut().record_host_function_call(
                        "wasi:clocks/monotonic-clock@0.2.3",
                        "now",
                        ().into_serializable_val(),
                        instant.into_serializable_val(),
                    );

                    Ok((instant,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap monotonic-clock now: {}", e))?;

        // resolution: func() -> duration (u64 nanoseconds)
        interface
            .func_wrap(
                "resolution",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    debug!("wasi:clocks/monotonic-clock resolution");
                    let duration = 1u64; // 1 nanosecond resolution

                    ctx.data_mut().record_host_function_call(
                        "wasi:clocks/monotonic-clock@0.2.3",
                        "resolution",
                        ().into_serializable_val(),
                        duration.into_serializable_val(),
                    );

                    Ok((duration,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap monotonic-clock resolution: {}", e))?;

        // subscribe-instant: func(when: instant) -> pollable
        interface
            .func_wrap(
                "subscribe-instant",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (when,): (u64,)| -> Result<(Resource<Pollable>,)> {
                    debug!("wasi:clocks/monotonic-clock subscribe-instant: {}", when);

                    let pollable = Pollable { deadline: when };
                    let pollable_handle = {
                        let mut table = ctx.data_mut().resource_table.lock().unwrap();
                        table.push(pollable)?
                    };
                    let pollable_id = pollable_handle.rep();

                    ctx.data_mut().record_host_function_call(
                        "wasi:clocks/monotonic-clock@0.2.3",
                        "subscribe-instant",
                        when.into_serializable_val(),
                        pollable_id.into_serializable_val(),
                    );

                    Ok((pollable_handle,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap monotonic-clock subscribe-instant: {}", e))?;

        // subscribe-duration: func(when: duration) -> pollable
        interface
            .func_wrap(
                "subscribe-duration",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (duration,): (u64,)| -> Result<(Resource<Pollable>,)> {
                    debug!("wasi:clocks/monotonic-clock subscribe-duration: {} ns", duration);

                    let now = Utc::now();
                    let created_at = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);
                    let deadline = created_at + duration;

                    let pollable = Pollable { deadline };
                    let pollable_handle = {
                        let mut table = ctx.data_mut().resource_table.lock().unwrap();
                        table.push(pollable)?
                    };
                    let pollable_id = pollable_handle.rep();

                    ctx.data_mut().record_host_function_call(
                        "wasi:clocks/monotonic-clock@0.2.3",
                        "subscribe-duration",
                        duration.into_serializable_val(),
                        val_serde::SerializableVal::Tuple(vec![
                            pollable_id.into_serializable_val(),
                            deadline.into_serializable_val(),
                        ]),
                    );

                    Ok((pollable_handle,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap monotonic-clock subscribe-duration: {}", e))?;

        info!("wasi:clocks/monotonic-clock@0.2.3 interface added");
        Ok(())
    }

    fn setup_poll(&self, actor_component: &mut ActorComponent) -> Result<()> {
        let mut interface = match actor_component
            .linker
            .instance("wasi:io/poll@0.2.3")
        {
            Ok(i) => i,
            Err(_) => {
                debug!("wasi:io/poll@0.2.3 not imported by component, skipping");
                return Ok(());
            }
        };

        // Register the pollable resource type
        interface
            .resource(
                "pollable",
                ResourceType::host::<Pollable>(),
                |mut ctx: StoreContextMut<'_, ActorStore>, rep: u32| {
                    debug!("wasi:io/poll pollable destructor called for rep={}", rep);
                    // Resource cleanup happens through [resource-drop]pollable
                    // This destructor is called by wasmtime when owned resources are dropped
                    Ok(())
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to register pollable resource: {}", e))?;

        // poll: func(in: list<borrow<pollable>>) -> list<u32>
        interface
            .func_wrap(
                "poll",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (pollables,): (Vec<Resource<Pollable>>,)| -> Result<(Vec<u32>,)> {
                    debug!("wasi:io/poll poll: {} pollables", pollables.len());

                    let pollable_ids: Vec<u32> = pollables.iter().map(|p| p.rep()).collect();

                    let now = Utc::now();
                    let current_instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

                    let mut ready_indices = Vec::new();
                    for (idx, pollable_handle) in pollables.iter().enumerate() {
                        let is_ready = {
                            let table = ctx.data_mut().resource_table.lock().unwrap();
                            if let Ok(pollable) = table.get(pollable_handle) {
                                current_instant >= pollable.deadline
                            } else {
                                false
                            }
                        };
                        if is_ready {
                            ready_indices.push(idx as u32);
                        }
                    }

                    ctx.data_mut().record_host_function_call(
                        "wasi:io/poll@0.2.3",
                        "poll",
                        pollable_ids.into_serializable_val(),
                        ready_indices.clone().into_serializable_val(),
                    );

                    Ok((ready_indices,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap poll: {}", e))?;

        // [method]pollable.ready: func() -> bool
        interface
            .func_wrap(
                "[method]pollable.ready",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (pollable_handle,): (Resource<Pollable>,)| -> Result<(bool,)> {
                    let pollable_id = pollable_handle.rep();
                    debug!("wasi:io/poll pollable.ready: {}", pollable_id);

                    let now = Utc::now();
                    let current_instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

                    let is_ready = {
                        let table = ctx.data_mut().resource_table.lock().unwrap();
                        if let Ok(pollable) = table.get(&pollable_handle) {
                            current_instant >= pollable.deadline
                        } else {
                            false
                        }
                    };

                    ctx.data_mut().record_host_function_call(
                        "wasi:io/poll@0.2.3",
                        "[method]pollable.ready",
                        pollable_id.into_serializable_val(),
                        is_ready.into_serializable_val(),
                    );

                    Ok((is_ready,))
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap pollable.ready: {}", e))?;

        // [method]pollable.block: func()
        interface
            .func_wrap_async(
                "[method]pollable.block",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (pollable_handle,): (Resource<Pollable>,)| {
                    let pollable_id = pollable_handle.rep();
                    debug!("wasi:io/poll pollable.block: {}", pollable_id);

                    Box::new(async move {
                        let deadline = {
                            let table = ctx.data_mut().resource_table.lock().unwrap();
                            if let Ok(pollable) = table.get(&pollable_handle) {
                                pollable.deadline
                            } else {
                                return Ok(());
                            }
                        };

                        let now = Utc::now();
                        let current_instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

                        if current_instant < deadline {
                            let sleep_nanos = deadline - current_instant;
                            let sleep_duration = std::time::Duration::from_nanos(sleep_nanos);
                            tokio::time::sleep(sleep_duration).await;
                        }

                        ctx.data_mut().record_host_function_call(
                            "wasi:io/poll@0.2.3",
                            "[method]pollable.block",
                            pollable_id.into_serializable_val(),
                            ().into_serializable_val(),
                        );

                        Ok(())
                    })
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap pollable.block: {}", e))?;

        // [resource-drop]pollable: func(self: pollable)
        interface
            .func_wrap(
                "[resource-drop]pollable",
                move |mut ctx: StoreContextMut<'_, ActorStore>, (pollable_handle,): (Resource<Pollable>,)| -> Result<()> {
                    let pollable_id = pollable_handle.rep();
                    debug!("wasi:io/poll [resource-drop]pollable: {}", pollable_id);

                    // Remove from resource table
                    {
                        let mut table = ctx.data_mut().resource_table.lock().unwrap();
                        let _ = table.delete(pollable_handle);
                    }

                    ctx.data_mut().record_host_function_call(
                        "wasi:io/poll@0.2.3",
                        "[resource-drop]pollable",
                        pollable_id.into_serializable_val(),
                        ().into_serializable_val(),
                    );

                    Ok(())
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to wrap [resource-drop]pollable: {}", e))?;

        info!("wasi:io/poll@0.2.3 interface added");
        Ok(())
    }
}

impl Handler for TimingHandler {
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        info!("Starting timing handler");
        Box::pin(async move {
            shutdown_receiver.wait_for_shutdown().await;
            info!("Timing handler received shutdown signal");
            Ok(())
        })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
        ctx: &mut HandlerContext,
    ) -> Result<()> {
        // Check if imports are already satisfied (e.g., by replay handler)
        let wall_clock_satisfied = ctx.is_satisfied("wasi:clocks/wall-clock@0.2.3");
        let monotonic_satisfied = ctx.is_satisfied("wasi:clocks/monotonic-clock@0.2.3");
        let poll_satisfied = ctx.is_satisfied("wasi:io/poll@0.2.3");

        info!(
            "Timing handler setup_host_functions called. Satisfied: wall={}, mono={}, poll={}",
            wall_clock_satisfied, monotonic_satisfied, poll_satisfied
        );

        if wall_clock_satisfied && monotonic_satisfied && poll_satisfied {
            info!("WASI clocks interfaces already satisfied (replay mode), skipping setup");
            return Ok(());
        }

        info!("Setting up WASI clocks host functions with manual func_wrap");

        if !wall_clock_satisfied {
            self.setup_wall_clock(actor_component)?;
            ctx.mark_satisfied("wasi:clocks/wall-clock@0.2.3");
        }

        if !monotonic_satisfied {
            self.setup_monotonic_clock(actor_component)?;
            ctx.mark_satisfied("wasi:clocks/monotonic-clock@0.2.3");
        }

        if !poll_satisfied {
            self.setup_poll(actor_component)?;
            ctx.mark_satisfied("wasi:io/poll@0.2.3");
        }

        info!("WASI clocks host functions setup complete");
        Ok(())
    }

    fn add_export_functions(
        &self,
        _actor_instance: &mut ActorInstance,
    ) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "timing"
    }

    fn imports(&self) -> Option<Vec<String>> {
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
