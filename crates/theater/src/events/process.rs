use serde::{Deserialize, Serialize};

/// Event data specific to the OS Process handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessEventData {
    /// Process was spawned
    ProcessSpawn {
        /// Process ID (within Theater)
        process_id: u64,
        /// Executable path
        program: String,
        /// Arguments
        args: Vec<String>,
        /// OS process ID
        os_pid: Option<u32>,
    },

    /// Process stdin write
    StdinWrite {
        /// Process ID
        process_id: u64,
        /// Number of bytes written
        bytes_written: u32,
    },

    /// Process stdout output
    StdoutOutput {
        /// Process ID
        process_id: u64,
        /// Total bytes in this output chunk
        bytes: usize,
    },

    /// Process stderr output
    StderrOutput {
        /// Process ID
        process_id: u64,
        /// Total bytes in this output chunk
        bytes: usize,
    },

    /// Process exit
    ProcessExit {
        /// Process ID
        process_id: u64,
        /// Exit code
        exit_code: i32,
    },

    /// Process signal sent
    SignalSent {
        /// Process ID
        process_id: u64,
        /// Signal number
        signal: u32,
    },

    /// Process kill request
    KillRequest {
        /// Process ID
        process_id: u64,
    },

    /// Error occurred
    Error {
        /// Process ID (if applicable)
        process_id: Option<u64>,
        /// Operation that failed
        operation: String,
        /// Error message
        message: String,
    },
    
    /// Permission denied
    PermissionDenied {
        /// Operation that was denied
        operation: String,
        /// Program that was denied
        program: String,
        /// Reason for denial
        reason: String,
    },

    /// Process timeout triggered
    TimeoutTriggered {
        /// Process ID
        process_id: u64,
        /// Timeout duration in seconds
        timeout_seconds: u64,
        /// Action taken (SIGTERM, SIGKILL, etc.)
        action: String,
    },

    /// Process timeout warning (optional future enhancement)
    TimeoutWarning {
        /// Process ID
        process_id: u64,
        /// Timeout duration in seconds
        timeout_seconds: u64,
        /// Warning threshold in seconds
        warning_seconds: u64,
    },

    // Handler setup events
    HandlerSetupStart,
    HandlerSetupSuccess,
    HandlerSetupError {
        error: String,
        step: String,
    },
    LinkerInstanceSuccess,
    FunctionSetupStart {
        function_name: String,
    },
    FunctionSetupSuccess {
        function_name: String,
    },
}
