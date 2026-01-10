//! Host trait implementations for WASI Clocks and Poll interfaces
//!
//! These implementations provide timing capabilities to actors while
//! recording all operations in the event chain for replay.

use crate::bindings::{WallClockHost, MonotonicClockHost, PollHost, HostPollable, Datetime};
use crate::{Pollable, PollableKind};
use anyhow::Result;
use chrono::Utc;
use wasmtime::component::Resource;
use theater::actor::ActorStore;
use tracing::debug;

// Implement WallClockHost for ActorStore
impl WallClockHost for ActorStore {
    async fn now(&mut self) -> Result<Datetime> {
        debug!("wasi:clocks/wall-clock now");

        let now = Utc::now();
        let seconds = now.timestamp() as u64;
        let nanoseconds = now.timestamp_subsec_nanos();

        // Record for replay - output is (seconds, nanoseconds)
        self.record_host_function_call(
            "wasi:clocks/wall-clock@0.2.3",
            "now",
            &(),
            &(seconds, nanoseconds),
        );

        Ok(Datetime { seconds, nanoseconds })
    }

    async fn resolution(&mut self) -> Result<Datetime> {
        debug!("wasi:clocks/wall-clock resolution");

        // Clock resolution: 1 nanosecond
        let seconds = 0u64;
        let nanoseconds = 1u32;

        // Record for replay
        self.record_host_function_call(
            "wasi:clocks/wall-clock@0.2.3",
            "resolution",
            &(),
            &(seconds, nanoseconds),
        );

        Ok(Datetime { seconds, nanoseconds })
    }
}

// Implement MonotonicClockHost for ActorStore
impl MonotonicClockHost for ActorStore {
    async fn now(&mut self) -> Result<u64> {
        debug!("wasi:clocks/monotonic-clock now");

        // Use UTC time converted to nanoseconds as an approximation of monotonic time
        let now = Utc::now();
        let instant = (now.timestamp() as u64) * 1_000_000_000 + (now.timestamp_subsec_nanos() as u64);

        // Record for replay
        self.record_host_function_call(
            "wasi:clocks/monotonic-clock@0.2.3",
            "now",
            &(),
            &instant,
        );

        Ok(instant)
    }

    async fn resolution(&mut self) -> Result<u64> {
        debug!("wasi:clocks/monotonic-clock resolution");

        // Clock resolution: 1 nanosecond
        let duration = 1u64;

        // Record for replay
        self.record_host_function_call(
            "wasi:clocks/monotonic-clock@0.2.3",
            "resolution",
            &(),
            &duration,
        );

        Ok(duration)
    }

    async fn subscribe_instant(&mut self, when: u64) -> Result<Resource<Pollable>> {
        debug!("wasi:clocks/monotonic-clock subscribe-instant: {}", when);

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

        // Record for replay - input is when, output is pollable_id
        self.record_host_function_call(
            "wasi:clocks/monotonic-clock@0.2.3",
            "subscribe-instant",
            &when,
            &pollable_id,
        );

        Ok(pollable_handle)
    }

    async fn subscribe_duration(&mut self, duration: u64) -> Result<Resource<Pollable>> {
        debug!("wasi:clocks/monotonic-clock subscribe-duration: {} ns", duration);

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

        // Record for replay - input is duration, output includes pollable_id and deadline for reconstruction
        self.record_host_function_call(
            "wasi:clocks/monotonic-clock@0.2.3",
            "subscribe-duration",
            &duration,
            &(pollable_id, deadline),
        );

        Ok(pollable_handle)
    }
}

// Implement PollHost for ActorStore (the poll function)
impl PollHost for ActorStore {
    async fn poll(&mut self, pollables: Vec<Resource<Pollable>>) -> Result<Vec<u32>> {
        debug!("wasi:io/poll poll: {} pollables", pollables.len());

        // Extract pollable IDs for recording
        let pollable_ids: Vec<u32> = pollables.iter().map(|p| p.rep()).collect();

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

        // Record for replay - input is pollable IDs, output is ready indices
        self.record_host_function_call(
            "wasi:io/poll@0.2.3",
            "poll",
            &pollable_ids,
            &ready_indices,
        );

        Ok(ready_indices)
    }
}

// Implement HostPollable for ActorStore (pollable resource methods)
impl HostPollable for ActorStore {
    async fn ready(&mut self, pollable_handle: Resource<Pollable>) -> Result<bool> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.ready: {}", pollable_id);

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

        // Record for replay - input is pollable_id, output is is_ready
        self.record_host_function_call(
            "wasi:io/poll@0.2.3",
            "[method]pollable.ready",
            &pollable_id,
            &is_ready,
        );

        Ok(is_ready)
    }

    async fn block(&mut self, pollable_handle: Resource<Pollable>) -> Result<()> {
        let pollable_id = pollable_handle.rep();
        debug!("wasi:io/poll pollable.block: {}", pollable_id);

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

        // Record for replay - input is pollable_id, output is unit
        self.record_host_function_call(
            "wasi:io/poll@0.2.3",
            "[method]pollable.block",
            &pollable_id,
            &(),
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
