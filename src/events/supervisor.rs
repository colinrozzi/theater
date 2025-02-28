use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisorEventData {
    // Spawn child events
    SpawnChildCall {
        manifest_path: String,
    },
    SpawnChildResult {
        child_id: String,
        success: bool,
    },

    // Stop child events
    StopChildCall {
        child_id: String,
    },
    StopChildResult {
        child_id: String,
        success: bool,
    },

    // Restart child events
    RestartChildCall {
        child_id: String,
    },
    RestartChildResult {
        child_id: String,
        success: bool,
    },

    // Get child state events
    GetChildStateCall {
        child_id: String,
    },
    GetChildStateResult {
        child_id: String,
        state_size: usize,
        success: bool,
    },

    // Get child events events
    GetChildEventsCall {
        child_id: String,
    },
    GetChildEventsResult {
        child_id: String,
        events_count: usize,
        success: bool,
    },

    // List children events
    ListChildrenCall {},
    ListChildrenResult {
        children_count: usize,
        success: bool,
    },

    // Error events
    Error {
        operation: String,
        child_id: Option<String>,
        message: String,
    },
}

pub struct SupervisorEvent {
    pub data: SupervisorEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
