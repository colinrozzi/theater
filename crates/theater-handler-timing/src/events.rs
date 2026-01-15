//! Timing handler event types for WASI clocks interfaces

use serde::{Deserialize, Serialize};

/// Event types for the WASI clocks handler.
///
/// This handler provides:
/// - `wasi:clocks/wall-clock@0.2.3` - Real-world wall clock time
/// - `wasi:clocks/monotonic-clock@0.2.3` - Monotonic time for measuring durations
/// - `wasi:io/poll@0.2.3` - Polling interface for async operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimingEventData {
    // WASI wall-clock events
    WallClockNowCall,
    WallClockNowResult { seconds: u64, nanoseconds: u32 },
    WallClockResolutionCall,
    WallClockResolutionResult { seconds: u64, nanoseconds: u32 },

    // WASI monotonic-clock events
    MonotonicClockNowCall,
    MonotonicClockNowResult { instant: u64 },
    MonotonicClockResolutionCall,
    MonotonicClockResolutionResult { duration: u64 },
    MonotonicClockSubscribeInstantCall { when: u64 },
    MonotonicClockSubscribeInstantResult { when: u64, pollable_id: u32 },
    MonotonicClockSubscribeDurationCall { duration: u64 },
    MonotonicClockSubscribeDurationResult { duration: u64, deadline: u64, pollable_id: u32 },

    // WASI poll events
    PollCall { num_pollables: usize },
    PollResult { ready_indices: Vec<u32> },
    PollableReadyCall { pollable_id: u32 },
    PollableReadyResult { pollable_id: u32, is_ready: bool },
    PollableBlockCall { pollable_id: u32 },
    PollableBlockResult { pollable_id: u32 },
}
