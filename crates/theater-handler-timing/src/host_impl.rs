//! Host trait implementations for WASI Clocks and Poll interfaces
//!
//! These implementations provide timing capabilities to actors while
//! recording all operations in the event chain for replay.

use crate::bindings::{WallClockHost, MonotonicClockHost, PollHost, HostPollable, Datetime};
use crate::events::TimingEventData;
use crate::{Pollable, PollableKind};
use anyhow::Result;
use chrono::Utc;
use wasmtime::component::Resource;
use theater::actor::ActorStore;
use theater::events::EventPayload;
use tracing::debug;

// Implement WallClockHost for ActorStore
impl<E> WallClockHost for ActorStore<E>
where
    E: EventPayload + Clone + From<TimingEventData> + Send,
{
    async fn now(&mut self) -> Result<Datetime> {
        debug!("wasi:clocks/wall-clock now");

        self.record_handler_event(
            "wasi:clocks/wall-clock/now".to_string(),
            TimingEventData::WallClockNowCall,
            Some("WASI wall-clock: requesting current time".to_string()),
        );

        let now = Utc::now();
        let seconds = now.timestamp() as u64;
        let nanoseconds = now.timestamp_subsec_nanos();

        // CRITICAL: Log the actual time values for replay
        self.record_handler_event(
            "wasi:clocks/wall-clock/now".to_string(),
            TimingEventData::WallClockNowResult { seconds, nanoseconds },
            Some(format!("WASI wall-clock: {}.{:09}s", seconds, nanoseconds)),
        );

        Ok(Datetime { seconds, nanoseconds })
    }

    async fn resolution(&mut self) -> Result<Datetime> {
        debug!("wasi:clocks/wall-clock resolution");

        self.record_handler_event(
            "wasi:clocks/wall-clock/resolution".to_string(),
            TimingEventData::WallClockResolutionCall,
            Some("WASI wall-clock: requesting resolution".to_string()),
        );

        // Clock resolution: 1 nanosecond
        let seconds = 0u64;
        let nanoseconds = 1u32;

        self.record_handler_event(
            "wasi:clocks/wall-clock/resolution".to_string(),
            TimingEventData::WallClockResolutionResult { seconds, nanoseconds },
            Some(format!("WASI wall-clock resolution: {}.{:09}s", seconds, nanoseconds)),
        );

        Ok(Datetime { seconds, nanoseconds })
    }
}

// Implement MonotonicClockHost for ActorStore
impl<E> MonotonicClockHost for ActorStore<E>
where
    E: EventPayload + Clone + From<TimingEventData> + Send,
{
    async fn now(&mut self) -> Result<u64> {
        debug!("wasi:clocks/monotonic-clock now");

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/now".to_string(),
            TimingEventData::MonotonicClockNowCall,
            Some("WASI monotonic-clock: requesting current instant".to_string()),
        );

        // Use UTC time converted to nanoseconds as an approximation of monotonic time
        let now = Utc::now();
        let instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

        // CRITICAL: Log the actual instant for replay
        self.record_handler_event(
            "wasi:clocks/monotonic-clock/now".to_string(),
            TimingEventData::MonotonicClockNowResult { instant },
            Some(format!("WASI monotonic-clock: {} ns", instant)),
        );

        Ok(instant)
    }

    async fn resolution(&mut self) -> Result<u64> {
        debug!("wasi:clocks/monotonic-clock resolution");

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/resolution".to_string(),
            TimingEventData::MonotonicClockResolutionCall,
            Some("WASI monotonic-clock: requesting resolution".to_string()),
        );

        // Clock resolution: 1 nanosecond
        let duration = 1u64;

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/resolution".to_string(),
            TimingEventData::MonotonicClockResolutionResult { duration },
            Some(format!("WASI monotonic-clock resolution: {} ns", duration)),
        );

        Ok(duration)
    }

    async fn subscribe_instant(&mut self, when: u64) -> Result<Resource<Pollable>> {
        debug!("wasi:clocks/monotonic-clock subscribe-instant: {}", when);

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/subscribe-instant".to_string(),
            TimingEventData::MonotonicClockSubscribeInstantCall { when },
            Some(format!("WASI monotonic-clock: subscribing to instant {}", when)),
        );

        // Create a pollable for this instant
        let pollable = Pollable {
            deadline: when,
            kind: PollableKind::MonotonicInstant(when),
        };

        // Push into the actor's resource table
        let pollable_handle = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(pollable)?
        };

        let pollable_id = pollable_handle.rep();

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/subscribe-instant".to_string(),
            TimingEventData::MonotonicClockSubscribeInstantResult { when, pollable_id },
            Some(format!("WASI monotonic-clock: created pollable {} for instant {}", pollable_id, when)),
        );

        Ok(pollable_handle)
    }

    async fn subscribe_duration(&mut self, duration: u64) -> Result<Resource<Pollable>> {
        debug!("wasi:clocks/monotonic-clock subscribe-duration: {} ns", duration);

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/subscribe-duration".to_string(),
            TimingEventData::MonotonicClockSubscribeDurationCall { duration },
            Some(format!("WASI monotonic-clock: subscribing to duration {} ns", duration)),
        );

        // Get current instant
        let now = Utc::now();
        let created_at = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

        // Calculate deadline
        let deadline = created_at + duration;

        // Create a pollable for this duration
        let pollable = Pollable {
            deadline,
            kind: PollableKind::MonotonicDuration { duration, created_at },
        };

        // Push into the actor's resource table
        let pollable_handle = {
            let mut table = self.resource_table.lock().unwrap();
            table.push(pollable)?
        };

        let pollable_id = pollable_handle.rep();

        self.record_handler_event(
            "wasi:clocks/monotonic-clock/subscribe-duration".to_string(),
            TimingEventData::MonotonicClockSubscribeDurationResult { duration, deadline, pollable_id },
            Some(format!("WASI monotonic-clock: created pollable {} for duration {} ns (deadline: {})", pollable_id, duration, deadline)),
        );

        Ok(pollable_handle)
    }
}

// Implement PollHost for ActorStore (the poll function)
impl<E> PollHost for ActorStore<E>
where
    E: EventPayload + Clone + From<TimingEventData> + Send,
{
    async fn poll(&mut self, pollables: Vec<Resource<Pollable>>) -> Result<Vec<u32>> {
        debug!("wasi:io/poll poll: {} pollables", pollables.len());

        self.record_handler_event(
            "wasi:io/poll/poll".to_string(),
            TimingEventData::PollCall { num_pollables: pollables.len() },
            Some(format!("WASI poll: polling {} pollables", pollables.len())),
        );

        // Get current monotonic time
        let now = Utc::now();
        let current_instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

        let mut ready_indices = Vec::new();

        // Check each pollable
        for (idx, pollable_handle) in pollables.iter().enumerate() {
            let is_ready = {
                let table = self.resource_table.lock().unwrap();
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

        self.record_handler_event(
            "wasi:io/poll/poll".to_string(),
            TimingEventData::PollResult { ready_indices: ready_indices.clone() },
            Some(format!("WASI poll: {} pollables ready", ready_indices.len())),
        );

        Ok(ready_indices)
    }
}

// Implement HostPollable for ActorStore (pollable resource methods)
impl<E> HostPollable for ActorStore<E>
where
    E: EventPayload + Clone + From<TimingEventData> + Send,
{
    async fn ready(&mut self, pollable_handle: Resource<Pollable>) -> Result<bool> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.ready: {}", pollable_id);

        self.record_handler_event(
            "wasi:io/poll/pollable.ready".to_string(),
            TimingEventData::PollableReadyCall { pollable_id },
            Some(format!("WASI poll: checking if pollable {} is ready", pollable_id)),
        );

        // Get current monotonic time
        let now = Utc::now();
        let current_instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

        let is_ready = {
            let table = self.resource_table.lock().unwrap();
            if let Ok(pollable) = table.get(&pollable_handle) {
                current_instant >= pollable.deadline
            } else {
                false
            }
        };

        self.record_handler_event(
            "wasi:io/poll/pollable.ready".to_string(),
            TimingEventData::PollableReadyResult { pollable_id, is_ready },
            Some(format!("WASI poll: pollable {} ready={}", pollable_id, is_ready)),
        );

        Ok(is_ready)
    }

    async fn block(&mut self, pollable_handle: Resource<Pollable>) -> Result<()> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.block: {}", pollable_id);

        self.record_handler_event(
            "wasi:io/poll/pollable.block".to_string(),
            TimingEventData::PollableBlockCall { pollable_id },
            Some(format!("WASI poll: blocking on pollable {}", pollable_id)),
        );

        // Get the deadline from the pollable
        let deadline = {
            let table = self.resource_table.lock().unwrap();
            if let Ok(pollable) = table.get(&pollable_handle) {
                pollable.deadline
            } else {
                return Err(anyhow::anyhow!("Pollable not found"));
            }
        };

        // Get current time
        let now = Utc::now();
        let current_instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

        // If not ready yet, sleep until the deadline
        if current_instant < deadline {
            let sleep_nanos = deadline - current_instant;
            let sleep_duration = std::time::Duration::from_nanos(sleep_nanos);
            tokio::time::sleep(sleep_duration).await;
        }

        self.record_handler_event(
            "wasi:io/poll/pollable.block".to_string(),
            TimingEventData::PollableBlockResult { pollable_id },
            Some(format!("WASI poll: pollable {} unblocked", pollable_id)),
        );

        Ok(())
    }

    async fn drop(&mut self, pollable_handle: Resource<Pollable>) -> Result<()> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.drop: {}", pollable_id);

        // Remove from resource table
        let mut table = self.resource_table.lock().unwrap();
        let _ = table.delete(pollable_handle);

        Ok(())
    }
}
