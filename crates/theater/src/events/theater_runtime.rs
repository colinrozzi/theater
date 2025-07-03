use crate::{store::ContentRef, ManifestConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TheaterRuntimeEventData {
    // Theater runtime lifecycle events
    ActorLoadCall {
        manifest: ManifestConfig,
    },

    ActorLoadResult {
        success: bool,
    },
    ActorLoadError {
        error: String,
    },

    /// Event indicating an actor component update has started
    ActorUpdateStart {
        /// Address of the new component
        new_component_address: String,
    },

    /// Event indicating an actor component update has completed successfully
    ActorUpdateComplete {
        /// Address of the new component
        new_component_address: String,
    },

    /// Event indicating an actor component update has failed
    ActorUpdateError {
        /// Address of the component that failed to update
        new_component_address: String,
        /// Error message describing the failure
        error: String,
    },
}

pub struct TheaterRuntimeEvent {
    pub data: TheaterRuntimeEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
