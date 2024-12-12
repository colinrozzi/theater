use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::{debug, info, warn, error};

// System event types beyond chain events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEventType {
    Runtime,
    Http,
    Actor,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: SystemEventType,
    pub component: String,
    pub message: String,
    pub related_hash: Option<String>,
}

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

pub fn log_system_event(event: SystemEvent) {
    let prefix = match event.event_type {
        SystemEventType::Runtime => "RUNTIME",
        SystemEventType::Http => "HTTP",
        SystemEventType::Actor => "ACTOR",
        SystemEventType::Error => "ERROR",
    };
    
    let hash_info = event.related_hash
        .map(|h| format!(" (chain: #{})", h))
        .unwrap_or_default();
    
    match event.event_type {
        SystemEventType::Error => error!("[{}] {} - {}{}", prefix, event.component, event.message, hash_info),
        _ => info!("[{}] {} - {}{}", prefix, event.component, event.message, hash_info)
    }
}
