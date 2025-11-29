//! # Timing Handler
//!
//! Provides timing capabilities to WebAssembly actors in the Theater system.
//! This handler allows actors to get the current time, sleep for durations,
//! and wait until specific deadlines.

use anyhow::Result;
use chrono::Utc;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tokio::time::{sleep, Duration};
use tracing::{info, error};
use wasmtime::StoreContextMut;

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::actor::types::ActorError;
use theater::config::actor_manifest::TimingHostConfig;
use theater::config::enforcement::PermissionChecker;
use theater::config::permissions::TimingPermissions;
use theater::events::timing::TimingEventData;
use theater::events::{ChainEventData, EventData};
use theater::handler::Handler;
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

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

    async fn setup_host_functions_impl(&mut self, actor_component: &mut ActorComponent) -> Result<()> {
        // Record setup start
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "timing-setup".to_string(),
            data: EventData::Timing(TimingEventData::HandlerSetupStart),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Starting timing host function setup".to_string()),
        });

        info!("Setting up timing host functions");

        let mut interface = match actor_component.linker.instance("theater:simple/timing") {
            Ok(interface) => {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "timing-setup".to_string(),
                    data: EventData::Timing(TimingEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "timing-setup".to_string(),
                    data: EventData::Timing(TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "linker_instance".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to create linker instance: {}", e)),
                });
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
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    let now = Utc::now().timestamp_millis() as u64;

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/now".to_string(),
                        data: EventData::Timing(TimingEventData::NowCall {}),
                        timestamp: now,
                        description: Some("Getting current timestamp".to_string()),
                    });

                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/now".to_string(),
                        data: EventData::Timing(TimingEventData::NowResult { timestamp: now }),
                        timestamp: now,
                        description: Some(format!("Current timestamp: {}", now)),
                    });

                    Ok((now,))
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "timing-setup".to_string(),
                    data: EventData::Timing(TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "now_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to wrap now function: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap now function: {}", e)
            })?;

        // Implementation of the sleep() function
        let permissions_clone = permissions.clone();
        let _ = interface
            .func_wrap_async(
                "sleep",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (duration,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let now = Utc::now().timestamp_millis() as u64;
                    
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/sleep".to_string(),
                        data: EventData::Timing(TimingEventData::SleepCall { duration }),
                        timestamp: now,
                        description: Some(format!("Sleeping for {} ms", duration)),
                    });
                    
                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone,
                        "sleep",
                        duration,
                    ) {
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/timing/permission-denied".to_string(),
                            data: EventData::Timing(TimingEventData::PermissionDenied {
                                operation: "sleep".to_string(),
                                reason: e.to_string(),
                            }),
                            timestamp: now,
                            description: Some(format!("Permission denied for sleep operation: {}", e)),
                        });
                        
                        return Box::new(futures::future::ready(Ok((Err(format!("Permission denied: {}", e)),))));
                    }
                    
                    let duration_clone = duration;
                    
                    Box::new(async move {
                        if duration_clone > 0 {
                            sleep(Duration::from_millis(duration_clone)).await;
                        }
                        
                        let end_time = Utc::now().timestamp_millis() as u64;
                        
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/timing/sleep".to_string(),
                            data: EventData::Timing(TimingEventData::SleepResult {
                                duration: duration_clone,
                                success: true,
                            }),
                            timestamp: end_time,
                            description: Some(format!("Successfully slept for {} ms", duration_clone)),
                        });
                        
                        Ok((Ok(()),))
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "timing-setup".to_string(),
                    data: EventData::Timing(TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "sleep_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to wrap sleep function: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap sleep function: {}", e)
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
                    
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/deadline".to_string(),
                        data: EventData::Timing(TimingEventData::DeadlineCall { timestamp }),
                        timestamp: now,
                        description: Some(format!("Waiting until timestamp: {}", timestamp)),
                    });
                    
                    if timestamp <= now {
                        let success_msg = "Deadline already passed, continuing immediately";
                        
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/timing/deadline".to_string(),
                            data: EventData::Timing(TimingEventData::DeadlineResult {
                                timestamp,
                                success: true,
                            }),
                            timestamp: now,
                            description: Some(success_msg.to_string()),
                        });
                        
                        return Box::new(futures::future::ready(Ok((Ok(()),))));
                    }
                    
                    let duration = timestamp - now;
                    
                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone2,
                        "deadline",
                        duration,
                    ) {
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/timing/permission-denied".to_string(),
                            data: EventData::Timing(TimingEventData::PermissionDenied {
                                operation: "deadline".to_string(),
                                reason: e.to_string(),
                            }),
                            timestamp: now,
                            description: Some(format!("Permission denied for deadline operation: {}", e)),
                        });
                        
                        return Box::new(futures::future::ready(Ok((Err(format!("Permission denied: {}", e)),))));
                    }
                    
                    let timestamp_clone = timestamp;
                    
                    Box::new(async move {
                        sleep(Duration::from_millis(duration)).await;
                        
                        let end_time = Utc::now().timestamp_millis() as u64;
                        let reached_deadline = end_time >= timestamp_clone;
                        
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "theater:simple/timing/deadline".to_string(),
                            data: EventData::Timing(TimingEventData::DeadlineResult {
                                timestamp: timestamp_clone,
                                success: reached_deadline,
                            }),
                            timestamp: end_time,
                            description: Some(format!(
                                "Deadline wait completed at {}. Target was {}", 
                                end_time, 
                                timestamp_clone
                            )),
                        });
                        
                        Ok((Ok(()),))
                    })
                },
            )
            .map_err(|e| {
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "timing-setup".to_string(),
                    data: EventData::Timing(TimingEventData::HandlerSetupError {
                        error: e.to_string(),
                        step: "deadline_function_wrap".to_string(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!("Failed to wrap deadline function: {}", e)),
                });
                anyhow::anyhow!("Failed to wrap deadline function: {}", e)
            })?;

        actor_component.actor_store.record_event(ChainEventData {
            event_type: "timing-setup".to_string(),
            data: EventData::Timing(TimingEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Timing host functions setup completed successfully".to_string()),
        });

        info!("Timing host functions added successfully");
        Ok(())
    }

    async fn add_export_functions_impl(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
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

impl Handler for TimingHandler {
    fn create_instance(&self) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn start(
        &mut self,
        actor_handle: ActorHandle,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let handler = self.clone();
        Box::pin(async move { handler.start_impl(actor_handle, shutdown_receiver).await })
    }

    fn setup_host_functions(
        &mut self,
        actor_component: &mut ActorComponent,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.setup_host_functions_impl(actor_component))
    }

    fn add_export_functions(
        &self,
        actor_instance: &mut ActorInstance,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.add_export_functions_impl(actor_instance))
    }

    fn name(&self) -> &str {
        "timing"
    }

    fn imports(&self) -> Option<String> {
        Some("theater:simple/timing".to_string())
    }

    fn exports(&self) -> Option<String> {
        None
    }
}
