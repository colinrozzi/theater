use crate::config::permissions::HandlerPermission;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TheaterRuntimeEventData {
    // Theater runtime lifecycle events
    ActorLoadCall,

    ActorLoadResult {
        success: bool,
    },

    ActorLoadError {
        error: String,
    },

    ActorSetupError {
        error: String,
    },

    ValidatingPermissions {
        /// The permissions being validated
        permissions: HandlerPermission,
    },
    CreatingPackage,
    CreatingHandlers,

    /// Event indicating an actor package update has started
    ActorUpdateStart {
        /// Address of the new package
        new_package_address: String,
    },

    /// Event indicating an actor package update has completed successfully
    ActorUpdateComplete {
        /// Address of the new package
        new_package_address: String,
    },

    /// Event indicating an actor package update has failed
    ActorUpdateError {
        /// Address of the package that failed to update
        new_package_address: String,
        /// Error message describing the failure
        error: String,
    },

    InstantiatingActor,
    InitializingState,
    ActorReady,
}

pub struct TheaterRuntimeEvent {
    pub data: TheaterRuntimeEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
