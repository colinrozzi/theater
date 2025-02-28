use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeEventData {
    Startup {
        config_summary: String,
    },
    Shutdown {
        reason: String,
    },
    StateChange {
        old_state: String,
        new_state: String,
    },
    Error {
        message: String,
        context: Option<String>,
    },
    Log {
        level: String,
        message: String,
    },
}

pub struct RuntimeEvent {
    pub data: RuntimeEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
