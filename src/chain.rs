use crate::chain_emitter::CHAIN_EMITTER;
use crate::logging::{ChainEvent, ChainEventType};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;

/// Represents a single event in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// The type and data of the event
    pub event: EventContent,
    /// Hash of the parent event, None for the first event
    pub parent: Option<String>,
}

/// The content of an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventContent {
    /// The type of event that occurred
    #[serde(rename = "type")]
    pub type_: String,
    /// The data associated with this event
    pub data: Value,
}

/// Manages a chain of events with their relationships
#[derive(Debug)]
pub struct HashChain {
    /// Hash of the most recent event
    head: Option<String>,
    /// Map of event hashes to their content
    entries: HashMap<String, Event>,
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

    /// Adds a new event to the chain
    /// Returns the hash of the new event
    pub fn add_event(&mut self, type_: String, data: Value) -> String {
        let event = Event {
            event: EventContent {
                type_,
                data: data.clone(),
            },
            parent: self.head.clone(),
        };

        // Create a hash of the event
        let serialized = serde_json::to_string(&event).expect("Failed to serialize event");
        let hash = format!("{:x}", md5::compute(serialized));

        debug!("Adding event to chain: {}", hash);
        
        // Emit logging event
        CHAIN_EMITTER.emit(ChainEvent {
            hash: hash.clone(),
            timestamp: Utc::now(),
            actor_name: self.actor_name.clone(),
            event_type: if self.head.is_none() {
                ChainEventType::Init
            } else {
                match event.event.type_.as_str() {
                    "actor_message" | "message_sent" => ChainEventType::Message,
                    _ => ChainEventType::StateTransition,
                }
            },
            data: data,
            parent: self.head.clone(),
        });

        // Store the event and update the head
        self.entries.insert(hash.clone(), event);
        self.head = Some(hash.clone());

        hash
    }

    /// Gets the hash of the most recent event
    pub fn get_head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    /// Gets an event by its hash
    pub fn get_event(&self, hash: &str) -> Option<&Event> {
        self.entries.get(hash)
    }

    /// Gets the complete chain as a vector of (hash, event) pairs
    /// The vector is ordered from most recent to oldest
    pub fn get_full_chain(&self) -> Vec<(String, Event)> {
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

    /// Gets all events of a specific type
    pub fn get_events_by_type(&self, type_: &str) -> Vec<(String, Event)> {
        self.entries
            .iter()
            .filter(|(_, event)| event.event.type_ == type_)
            .map(|(hash, event)| (hash.clone(), event.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_basic_chain_operations() {
        let mut chain = HashChain::new();
        chain.set_actor_name("test-actor".to_string());
        
        // Add first event
        let hash1 = chain.add_event(
            "state_change".to_string(),
            json!({ "new_state": "value1" }),
        );
        assert_eq!(chain.get_head(), Some(&hash1));
        
        // Add second event
        let hash2 = chain.add_event(
            "message_received".to_string(),
            json!({ "content": "hello" }),
        );
        
        // Check parent relationship
        let event2 = chain.get_event(&hash2).unwrap();
        assert_eq!(event2.parent, Some(hash1));
        
        // Check full chain
        let full_chain = chain.get_full_chain();
        assert_eq!(full_chain.len(), 2);
        assert_eq!(full_chain[0].0, hash2);
        assert_eq!(full_chain[1].0, hash1);
    }
}