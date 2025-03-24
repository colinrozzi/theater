use crate::store::ContentRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TheaterRuntimeEventData {
    // Theater runtime lifecycle events
    ActorLoadCall { manifest_id: ContentRef },

    ActorLoadResult { success: bool },
    ActorLoadError { error: String },
}

pub struct TheaterRuntimeEvent {
    pub data: TheaterRuntimeEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
