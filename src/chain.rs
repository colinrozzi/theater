use crate::chain_emitter::CHAIN_EMITTER;
use crate::logging::{ChainEvent, ChainEventType};
use chrono::Utc;
use md5;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    pub parent: Option<String>,
    pub data: Value,
}

#[derive(Debug)]
pub struct HashChain {
    head: Option<String>,
    entries: HashMap<String, ChainEntry>,
}

impl HashChain {
    pub fn new() -> Self {
        Self {
            head: None,
            entries: HashMap::new(),
        }
    }

    pub fn add(&mut self, data: Value) -> String {
        let entry = ChainEntry {
            parent: self.head.clone(),
            data: data.clone(),
        };

        // Calculate hash of entry
        let serialized = serde_json::to_string(&entry).expect("Failed to serialize entry");
        let hash = format!("{:x}", md5::compute(serialized));

        // Create and log chain event
        let event = ChainEvent {
            hash: hash.clone(),
            timestamp: Utc::now(),
            actor_name: data
                .get("actor_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            event_type: if self.head.is_none() {
                ChainEventType::Init
            } else {
                ChainEventType::StateTransition
            },
            data,
            parent: self.head.clone(),
        };
        CHAIN_EMITTER.emit(event.clone());
        debug!("Chain event logged: #{}", event.hash);

        // Store entry and update head
        self.entries.insert(hash.clone(), entry);
        self.head = Some(hash.clone());

        hash
    }

    pub fn get_head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    pub fn get_full_chain(&self) -> Vec<(String, ChainEntry)> {
        let mut result = Vec::new();
        let mut current = self.head.clone();

        while let Some(hash) = current {
            let entry = self.entries.get(&hash).expect("Chain corrupted").clone();
            result.push((hash.clone(), entry.clone()));
            current = entry.parent;
        }

        result
    }
}
