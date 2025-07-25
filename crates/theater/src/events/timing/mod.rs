use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimingEventData {
    NowCall {},
    NowResult { timestamp: u64 },
    SleepCall { duration: u64 },
    SleepResult { duration: u64, success: bool },
    DeadlineCall { timestamp: u64 },
    DeadlineResult { timestamp: u64, success: bool },
    Error { operation: String, message: String },
    PermissionDenied { operation: String, reason: String },

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
