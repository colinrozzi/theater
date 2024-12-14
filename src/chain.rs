use crate::chain_emitter::CHAIN_EMITTER;
use crate::logging::ChainEventType;
use crate::{ActorInput, ActorOutput};
use chrono::Utc;
use md5;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainEvent {
    ExternalInput {
        input: ActorInput,
        timestamp: chrono::DateTime<Utc>,
    },
    ActorMessage {
        source_actor: String,
        source_chain_state: String,
        content: Value,
        timestamp: chrono::DateTime<Utc>,
    },
    StateChange {
        old_state: Value,
        new_state: Value,
        timestamp: chrono::DateTime<Utc>,
    },
    Output {
        output: ActorOutput,
        chain_state: String,
        timestamp: chrono::DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEntry {
    pub parent: Option<String>,
    pub event: ChainEvent,
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

    pub fn add_event(&mut self, event: ChainEvent) -> String {
        let entry = ChainEntry {
            parent: self.head.clone(),
            event,
        };

        let serialized = serde_json::to_string(&entry).expect("Failed to serialize entry");
        let hash = format!("{:x}", md5::compute(serialized));

        // Emit logging event
        CHAIN_EMITTER.emit(crate::logging::ChainEvent {
            hash: hash.clone(),
            timestamp: Utc::now(),
            actor_name: "unknown".to_string(), // TODO: Store actor name in chain
            event_type: if self.head.is_none() {
                ChainEventType::Init
            } else {
                ChainEventType::StateTransition
            },
            data: serde_json::to_value(&entry).unwrap(),
            parent: self.head.clone(),
        });

        debug!("Chain event logged: #{}", hash);

        self.entries.insert(hash.clone(), entry);
        self.head = Some(hash.clone());

        hash
    }

    pub fn get_head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    pub fn get_current_state(&self) -> Option<Value> {
        let mut current = self.head.as_ref()?;
        
        while let Some(entry) = self.entries.get(current) {
            if let ChainEvent::StateChange { new_state, .. } = &entry.event {
                return Some(new_state.clone());
            }
            if let Some(parent) = &entry.parent {
                current = parent;
            } else {
                break;
            }
        }
        None
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