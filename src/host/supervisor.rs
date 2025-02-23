use crate::actor_executor::ActorError;
use crate::actor_handle::ActorHandle;
use crate::config::SupervisorHostConfig;
use crate::wasm::Event;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{error, info};

pub struct SupervisorHost {
    #[allow(dead_code)]
    actor_handle: ActorHandle,
}

#[derive(Error, Debug)]
pub enum SupervisorError {
    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Actor error: {0}")]
    ActorError(#[from] ActorError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct SupervisorEvent {
    event_type: String,
    actor_id: String,
    data: Option<Vec<u8>>,
}

impl SupervisorHost {
    pub fn new(_config: SupervisorHostConfig, actor_handle: ActorHandle) -> Self {
        Self { actor_handle }
    }

    pub async fn setup_host_functions(&self) -> Result<()> {
        info!("Setting up host functions for supervisor");
        Ok(())
    }

    pub async fn add_exports(&self) -> Result<()> {
        info!("Adding exports for supervisor");
        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting supervisor host");
        Ok(())
    }

    async fn handle_supervisor_event(&self, event: SupervisorEvent) -> Result<(), SupervisorError> {
        // Create event for actor
        let event = Event {
            event_type: event.event_type.clone(),
            parent: None,
            data: serde_json::to_vec(&event)?,
        };

        // Send event to actor
        self.actor_handle.handle_event(event).await?;

        Ok(())
    }
}
