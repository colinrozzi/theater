use crate::wasm::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info};

/// Represents a single event in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    /// The type and data of the event
    pub event: Event,
    /// Hash of the parent event, None for the first event
    pub parent: Option<String>,
}

/// Manages a chain of events with their relationships
#[derive(Debug, Clone)]
pub struct HashChain {
    /// Hash of the most recent event
    head: Option<String>,
    /// Map of event hashes to their content
    entries: HashMap<String, ChainEntry>,
    /// Name of the actor that owns this chain
    actor_name: String,
}

impl HashChain {
    /// Creates a new, empty hash chain
    pub fn new() -> Self {
        Self {
            head: None,
            entries: HashMap::new(),
            actor_name: "unknown".to_string(),
        }
    }

    /// Set the actor name for this chain
    pub fn set_actor_name(&mut self, name: String) {
        self.actor_name = name;
    }

    pub fn add(&mut self, event: Event) -> String {
        info!("Adding event to chain: {:?}", event);
        let chain_entry = ChainEntry {
            event: event.clone(),
            parent: self.head.clone(),
        };
        let serialized = serde_json::to_string(&chain_entry).unwrap();
        let hash = format!("{:x}", md5::compute(serialized));

        debug!("Adding event to chain: {}", hash);

        // Store the event and update the head
        self.entries.insert(hash.clone(), chain_entry);
        self.head = Some(hash.clone());

        hash
    }

    /// Adds a new event to the chain
    /// Returns the hash of the new event
    pub fn add_event(&mut self, type_: String, data: Value) -> String {
        let event = Event { type_, data };

        self.add(event)
    }

    /// Gets the hash of the most recent event
    pub fn get_head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    /// Gets an event by its hash
    pub fn get_chain_entry(&self, hash: &str) -> Option<&ChainEntry> {
        self.entries.get(hash)
    }

    /// Gets the complete chain as a vector of (hash, event) pairs
    /// The vector is ordered from most recent to oldest
    pub fn get_full_chain(&self) -> Vec<(String, ChainEntry)> {
        let mut result = Vec::new();
        let mut current = self.head.clone();

        while let Some(hash) = current {
            if let Some(event) = self.entries.get(&hash) {
                result.push((hash.clone(), event.clone()));
                current = event.parent.clone();
            } else {
                break;
            }
        }

        result
    }
}
