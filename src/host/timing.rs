use crate::actor::types::ActorError;
use crate::actor::handle::ActorHandle;
use crate::actor::store::ActorStore;
use crate::config::TimingHostConfig;
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

pub struct TimingHost {
    config: TimingHostConfig,
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
    pub fn new(config: TimingHostConfig) -> Self {
        Self { config }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up timing host functions");

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/timing")
            .expect("Could not instantiate ntwk:theater/timing");

        let max_sleep_duration = self.config.max_sleep_duration;
        let min_sleep_duration = self.config.min_sleep_duration;

        // Implementation of the now() function
        let _ = interface
            .func_wrap(
                "now",
                move |mut ctx: StoreContextMut<'_, ActorStore>, ()| -> Result<(u64,)> {
                    let now = Utc::now().timestamp_millis() as u64;
                    
                    // Record now call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/timing/now".to_string(),
                        data: EventData::Timing(TimingEventData::NowCall {}),
                        timestamp: now,
                        description: Some("Getting current timestamp".to_string()),
                    });
                    
                    // Record now result event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/timing/now".to_string(),
                        data: EventData::Timing(TimingEventData::NowResult { timestamp: now }),
                        timestamp: now,
                        description: Some(format!("Current timestamp: {}", now)),
                    });
                    
                    Ok((now,))
                },
            )
            .expect("Failed to wrap now function");

        // Implementation of the sleep() function
        let _ = interface
            .func_wrap_async(
                "sleep",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (duration,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let now = Utc::now().timestamp_millis() as u64;
                    
                    // Record sleep call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/timing/sleep".to_string(),
                        data: EventData::Timing(TimingEventData::SleepCall { duration }),
                        timestamp: now,
                        description: Some(format!("Sleeping for {} ms", duration)),
                    });
                    
                    let max_duration = max_sleep_duration;
                    let min_duration = min_sleep_duration;
                    
                    // Check duration constraints
                    if duration > max_duration {
                        let error_msg = format!("Duration too long: {} ms exceeds maximum of {} ms", duration, max_duration);
                        
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/timing/sleep".to_string(),
                            data: EventData::Timing(TimingEventData::Error {
                                operation: "sleep".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: now,
                            description: Some(error_msg.clone()),
                        });
                        
                        return Box::new(futures::future::ready(Ok((Err(error_msg),))));
                    }
                    
                    if duration < min_duration && duration > 0 {
                        let error_msg = format!("Duration too short: {} ms is below minimum of {} ms", duration, min_duration);
                        
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/timing/sleep".to_string(),
                            data: EventData::Timing(TimingEventData::Error {
                                operation: "sleep".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: now,
                            description: Some(error_msg.clone()),
                        });
                        
                        return Box::new(futures::future::ready(Ok((Err(error_msg),))));
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
                            event_type: "ntwk:theater/timing/sleep".to_string(),
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
            .expect("Failed to wrap sleep function");

        // Implementation of the deadline() function
        let _ = interface
            .func_wrap_async(
                "deadline",
                move |mut ctx: StoreContextMut<'_, ActorStore>,
                      (timestamp,): (u64,)|
                      -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                    let now = Utc::now().timestamp_millis() as u64;
                    
                    // Record deadline call event
                    ctx.data_mut().record_event(ChainEventData {
                        event_type: "ntwk:theater/timing/deadline".to_string(),
                        data: EventData::Timing(TimingEventData::DeadlineCall { timestamp }),
                        timestamp: now,
                        description: Some(format!("Waiting until timestamp: {}", timestamp)),
                    });
                    
                    // Check if the deadline is in the past
                    if timestamp <= now {
                        let success_msg = "Deadline already passed, continuing immediately";
                        
                        // Record deadline result event (immediate success)
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/timing/deadline".to_string(),
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
                    let max_duration = max_sleep_duration;
                    
                    // Check if the duration exceeds the maximum
                    if duration > max_duration {
                        let error_msg = format!(
                            "Deadline too far in the future: {} ms from now exceeds maximum of {} ms",
                            duration, max_duration
                        );
                        
                        // Record error event
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "ntwk:theater/timing/deadline".to_string(),
                            data: EventData::Timing(TimingEventData::Error {
                                operation: "deadline".to_string(),
                                message: error_msg.clone(),
                            }),
                            timestamp: now,
                            description: Some(error_msg.clone()),
                        });
                        
                        return Box::new(futures::future::ready(Ok((Err(error_msg),))));
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
                            event_type: "ntwk:theater/timing/deadline".to_string(),
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
            .expect("Failed to wrap deadline function");

        info!("Timing host functions added successfully");
        Ok(())
    }

    pub async fn add_export_functions(&self, _actor_instance: &mut ActorInstance) -> Result<()> {
        info!("No export functions needed for timing host");
        Ok(())
    }

    pub async fn start(&self, _actor_handle: ActorHandle, _shutdown_receiver: ShutdownReceiver) -> Result<()> {
        info!("Starting timing host");
        Ok(())
    }
}
