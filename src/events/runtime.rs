use super::ChainEventData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeEvent {
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

impl ChainEventData for RuntimeEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Startup { .. } => "runtime.startup",
            Self::Shutdown { .. } => "runtime.shutdown",
            Self::StateChange { .. } => "runtime.state_change",
            Self::Error { .. } => "runtime.error",
            Self::Log { .. } => "runtime.log",
        }
    }
    
    fn description(&self) -> String {
        match self {
            Self::Startup { config_summary } => {
                format!("Runtime started with config: {}", config_summary)
            },
            Self::Shutdown { reason } => {
                format!("Runtime shutdown: {}", reason)
            },
            Self::StateChange { old_state, new_state } => {
                format!("State changed from '{}' to '{}'", old_state, new_state)
            },
            Self::Error { message, context } => {
                if let Some(ctx) = context {
                    format!("Runtime error in {}: {}", ctx, message)
                } else {
                    format!("Runtime error: {}", message)
                }
            },
            Self::Log { level, message } => {
                format!("[{}] {}", level, message)
            },
        }
    }
}
