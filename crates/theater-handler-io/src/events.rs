//! Event types for WASI I/O operations
//!
//! These events are logged to Theater's event chain to track all I/O operations
//! for replay and verification purposes.

use serde::{Deserialize, Serialize};

/// Event data for WASI I/O operations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IoEventData {
    /// input-stream.read called
    InputStreamReadCall {
        len: u64,
    },

    /// input-stream.read result
    InputStreamReadResult {
        bytes_read: usize,
        success: bool,
    },

    /// input-stream.skip called
    InputStreamSkipCall {
        len: u64,
    },

    /// input-stream.skip result
    InputStreamSkipResult {
        bytes_skipped: u64,
        success: bool,
    },

    /// output-stream.check-write called
    OutputStreamCheckWriteCall,

    /// output-stream.check-write result
    OutputStreamCheckWriteResult {
        available: u64,
        success: bool,
    },

    /// output-stream.write called
    OutputStreamWriteCall {
        len: usize,
    },

    /// output-stream.write result
    OutputStreamWriteResult {
        bytes_written: usize,
        success: bool,
    },

    /// output-stream.flush called
    OutputStreamFlushCall,

    /// output-stream.flush result
    OutputStreamFlushResult {
        success: bool,
    },

    /// error.to-debug-string called
    ErrorToDebugStringCall,

    /// error.to-debug-string result
    ErrorToDebugStringResult {
        debug_string: String,
    },

    /// stdin.get-stdin called
    StdinGetCall,

    /// stdin.get-stdin result
    StdinGetResult,

    /// stdout.get-stdout called
    StdoutGetCall,

    /// stdout.get-stdout result
    StdoutGetResult,

    /// stderr.get-stderr called
    StderrGetCall,

    /// stderr.get-stderr result
    StderrGetResult,

    /// environment.get-environment called
    EnvironmentGetCall,

    /// environment.get-environment result
    EnvironmentGetResult {
        count: usize,
    },

    /// exit.exit called
    ExitCall {
        status: u8,
    },

    /// output-stream.write-zeroes called
    OutputStreamWriteZeroesCall {
        len: u64,
    },

    /// output-stream.write-zeroes result
    OutputStreamWriteZeroesResult {
        bytes_written: u64,
        success: bool,
    },

    /// output-stream.splice called
    OutputStreamSpliceCall {
        len: u64,
    },

    /// output-stream.splice result
    OutputStreamSpliceResult {
        bytes_spliced: u64,
        success: bool,
    },

    /// input-stream.subscribe called
    InputStreamSubscribeCall,

    /// input-stream.subscribe result
    InputStreamSubscribeResult {
        pollable_id: u32,
    },

    /// output-stream.subscribe called
    OutputStreamSubscribeCall,

    /// output-stream.subscribe result
    OutputStreamSubscribeResult {
        pollable_id: u32,
    },

    /// poll called
    PollCall {
        num_pollables: usize,
    },

    /// poll result
    PollResult {
        ready_indices: Vec<u32>,
    },

    /// pollable.ready called
    PollableReadyCall {
        pollable_id: u32,
    },

    /// pollable.ready result
    PollableReadyResult {
        pollable_id: u32,
        is_ready: bool,
    },

    /// pollable.block called
    PollableBlockCall {
        pollable_id: u32,
    },

    /// pollable.block result
    PollableBlockResult {
        pollable_id: u32,
    },

    /// cli/arguments.get-arguments called
    ArgumentsGetCall,

    /// cli/arguments.get-arguments result
    ArgumentsGetResult {
        count: usize,
    },

    /// cli/initial-cwd called (the interface is just "initial-cwd", a function)
    InitialCwdCall,

    /// cli/initial-cwd result
    InitialCwdResult {
        cwd: Option<String>,
    },
}
