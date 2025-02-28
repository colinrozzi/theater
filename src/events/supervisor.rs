use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorEventData {
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

pub struct SupervisorEvent {
    pub data: SupervisorEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
