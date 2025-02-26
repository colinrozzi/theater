use super::ChainEventData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorEvent {
    ChildSpawned {
        child_id: String,
        manifest_path: String,
    },
    ChildStopped {
        child_id: String,
        reason: String,
    },
    ChildRestarted {
        child_id: String,
        reason: String,
    },
    Error {
        message: String,
        child_id: Option<String>,
    },
}

impl ChainEventData for SupervisorEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::ChildSpawned { .. } => "supervisor.child_spawned",
            Self::ChildStopped { .. } => "supervisor.child_stopped",
            Self::ChildRestarted { .. } => "supervisor.child_restarted",
            Self::Error { .. } => "supervisor.error",
        }
    }
    
    fn description(&self) -> String {
        match self {
            Self::ChildSpawned { child_id, manifest_path } => {
                format!("Spawned child actor {} from {}", child_id, manifest_path)
            },
            Self::ChildStopped { child_id, reason } => {
                format!("Stopped child actor {}: {}", child_id, reason)
            },
            Self::ChildRestarted { child_id, reason } => {
                format!("Restarted child actor {}: {}", child_id, reason)
            },
            Self::Error { message, child_id } => {
                if let Some(id) = child_id {
                    format!("Supervisor error for child {}: {}", id, message)
                } else {
                    format!("Supervisor error: {}", message)
                }
            },
        }
    }
}
