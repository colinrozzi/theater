use crate::chain::ChainEvent;
use serde::{Deserialize, Serialize};

/// Base trait for all chain events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEventData {
    pub event_type: String,
    pub data: EventData,
    pub timestamp: u64,
    // Optional human-readable description
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventData {
    Filesystem(filesystem::FilesystemEventData),
    Http(http::HttpEventData),
    Message(message::MessageEventData),
    Runtime(runtime::RuntimeEventData),
    Supervisor(supervisor::SupervisorEventData),
    Store(store::StoreEventData),
    Timing(timing::TimingEventData),
    Wasm(wasm::WasmEventData),
}

impl ChainEventData {
    /// The event type identifier
    #[allow(dead_code)]
    fn event_type(&self) -> String {
        let event_type = self.event_type.clone();
        event_type
    }

    /// Human-readable description of the event
    #[allow(dead_code)]
    fn description(&self) -> String {
        match &self.description {
            Some(desc) => desc.clone(),
            None => String::from(""),
        }
    }

    /// Convert to JSON
    #[allow(dead_code)]
    fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn to_chain_event(&self, parent_hash: Option<Vec<u8>>) -> ChainEvent {
        ChainEvent {
            parent_hash,
            hash: vec![],
            event_type: self.event_type.clone(),
            data: serde_json::to_vec(&self.data).unwrap_or_else(|_| vec![]),
            timestamp: self.timestamp,
            description: self.description.clone(),
        }
    }
}

pub mod filesystem;
pub mod http;
pub mod message;
pub mod runtime;
pub mod store;
pub mod supervisor;
pub mod timing;
pub mod wasm;
