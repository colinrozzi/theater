//! Timing handler event types

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimingEventData {
    // Theater simple/timing events
    NowCall {},
    NowResult { timestamp: u64 },
    SleepCall { duration: u64 },
    SleepResult { duration: u64, success: bool },
    DeadlineCall { timestamp: u64 },
    DeadlineResult { timestamp: u64, success: bool },
    Error { operation: String, message: String },
    PermissionDenied { operation: String, reason: String },

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

    // Handler setup events
    HandlerSetupStart,
    HandlerSetupSuccess,
    HandlerSetupError { error: String, step: String },
    LinkerInstanceSuccess,
    FunctionSetupStart { function_name: String },
    FunctionSetupSuccess { function_name: String },
}
