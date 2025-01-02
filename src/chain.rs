use crate::process::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
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
}

impl HashChain {
    /// Creates a new, empty hash chain
    pub fn new() -> Self {
        Self {
            head: None,
            entries: HashMap::new(),
        }
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
