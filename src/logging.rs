use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEvent {
    pub hash: String,
    pub timestamp: DateTime<Utc>,
    pub actor_name: String,
    pub event_type: ChainEventType,
    pub data: serde_json::Value,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainEventType {
    Init,
    StateTransition,
    Message,
}

impl fmt::Display for ChainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "----------------------------------")?;
        writeln!(f, "CHAIN COMMIT #{}", self.hash)?;
        writeln!(f, "TIMESTAMP: {}", self.timestamp.to_rfc3339())?;
        writeln!(f, "ACTOR: {}", self.actor_name)?;
        writeln!(f, "TYPE: {:?}", self.event_type)?;
        writeln!(f, "DATA:\n{}", serde_json::to_string_pretty(&self.data).unwrap())?;
        if let Some(parent) = &self.parent {
            writeln!(f, "PARENT: #{}", parent)?;
        }
        writeln!(f, "----------------------------------")
    }
}

pub fn log_chain_event(event: &ChainEvent) {
    info!("\n{}", event);
    debug!("Chain event logged: #{}", event.hash);
}
