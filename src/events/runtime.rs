use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeEventData {
    // Actor lifecycle events
    InitCall {
        params: String,
    },
    InitResult {
        success: bool,
    },

    // Runtime lifecycle events
    StartupCall {
        config_summary: String,
    },
    StartupResult {
        success: bool,
    },

    ShutdownCall {
        reason: String,
    },
    ShutdownResult {
        success: bool,
    },

    // State change events
    StateChangeCall {
        old_state: String,
        new_state: String,
    },
    StateChangeResult {
        success: bool,
    },

    // Log events (these don't typically have call/result pairs)
    Log {
        level: String,
        message: String,
    },

    // Error events
    Error {
        operation: String,
        message: String,
        context: Option<String>,
    },
}

pub struct RuntimeEvent {
    pub data: RuntimeEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
