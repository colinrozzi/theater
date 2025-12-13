use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::actor::types::ActorError;
use crate::config::actor_manifest::TimingHostConfig;
use crate::config::enforcement::PermissionChecker;
use crate::events::timing::TimingEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};
use anyhow::Result;
use chrono::Utc;
use std::future::Future;
use thiserror::Error;
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use wasmtime::StoreContextMut;

#[derive(Clone)]
pub struct TimingHost {
    #[allow(dead_code)]
    config: TimingHostConfig,
    permissions: Option<crate::config::permissions::TimingPermissions>,
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

impl TimingHost {
    pub fn new(
        config: TimingHostConfig,
        permissions: Option<crate::config::permissions::TimingPermissions>,
    ) -> Self {
        Self {
            config,
            permissions,
        }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
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
                // Record successful linker instance creation
                actor_component.actor_store.record_event(ChainEventData {
                    event_type: "timing-setup".to_string(),
                    data: EventData::Timing(TimingEventData::LinkerInstanceSuccess),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some("Successfully created linker instance".to_string()),
                });
                interface
            }
            Err(e) => {
                // Record the specific error where it happens
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

                    // Record now call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/now".to_string(),
                        data: EventData::Timing(TimingEventData::NowCall {}),
                        timestamp: now,
                        description: Some("Getting current timestamp".to_string()),
                    });

                    // Record now result event
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
                // Record function setup error
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
                    
                    // Record sleep call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/sleep".to_string(),
                        data: EventData::Timing(TimingEventData::SleepCall { duration }),
                        timestamp: now,
                        description: Some(format!("Sleeping for {} ms", duration)),
                    });
                    
                    // PERMISSION CHECK BEFORE OPERATION
                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone,
                        "sleep",
                        duration,
                    ) {
                        // Record permission denied event
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
                    
                    // Clone duration for the closure
                    let duration_clone = duration;
                    
                    Box::new(async move {
                        if duration_clone > 0 {
                            sleep(Duration::from_millis(duration_clone)).await;
                        }
                        
                        let end_time = Utc::now().timestamp_millis() as u64;
                        
                        // Record sleep result event
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
                // Record function setup error
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
                    
                    // Record deadline call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "theater:simple/timing/deadline".to_string(),
                        data: EventData::Timing(TimingEventData::DeadlineCall { timestamp }),
                        timestamp: now,
                        description: Some(format!("Waiting until timestamp: {}", timestamp)),
                    });
                    
                    // Check if the deadline is in the past
                    if timestamp <= now {
                        let success_msg = "Deadline already passed, continuing immediately";
                        
                        // Record deadline result event (immediate success)
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
                    
                    // Calculate the duration to sleep
                    let duration = timestamp - now;
                    
                    // PERMISSION CHECK BEFORE OPERATION
                    if let Err(e) = PermissionChecker::check_timing_operation(
                        &permissions_clone2,
                        "deadline",
                        duration,
                    ) {
                        // Record permission denied event
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
                    
                    // Clone timestamp for the closure
                    let timestamp_clone = timestamp;
                    
                    Box::new(async move {
                        // Sleep until the deadline or max duration
                        sleep(Duration::from_millis(duration)).await;
                        
                        let end_time = Utc::now().timestamp_millis() as u64;
                        let reached_deadline = end_time >= timestamp_clone;
                        
                        // Record deadline result event
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
                // Record function setup error
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

        // Record overall setup completion
        actor_component.actor_store.record_event(ChainEventData {
            event_type: "timing-setup".to_string(),
            data: EventData::Timing(TimingEventData::HandlerSetupSuccess),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            description: Some("Timing host functions setup completed successfully".to_string()),
        });

        info!("Timing host functions added successfully");
        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No export functions needed for timing host");
        Ok(())
    }

    pub async fn start(
        &self,
        _actor_handle: ActorHandle,
        _shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        info!("Starting timing host");
        Ok(())
    }
}
