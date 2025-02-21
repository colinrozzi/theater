use serde::{Deserialize, Serialize};
use sha1::{Sha1, Digest};
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEvent {
    pub hash: Vec<u8>,
    pub parent_hash: Option<Vec<u8>>,
    pub event_type: String,
    pub data: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChain {
    events: Vec<ChainEvent>,
    current_hash: Option<Vec<u8>>,
}

impl StateChain {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            current_hash: None,
        }
    }

    pub fn add_event(&mut self, event_type: String, data: Vec<u8>) -> ChainEvent {
        let mut hasher = Sha1::new();
        
        // Hash previous state + new event data
        if let Some(prev_hash) = &self.current_hash {
            hasher.update(prev_hash);
        }
        hasher.update(&data);
        
        let hash = hasher.finalize().to_vec();
        
        let event = ChainEvent {
            hash: hash.clone(),
            parent_hash: self.current_hash.clone(),
            event_type,
            data,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        self.events.push(event.clone());
        self.current_hash = Some(hash);
        
        event
    }

    pub fn verify(&self) -> bool {
        let mut prev_hash = None;
        
        for event in &self.events {
            let mut hasher = Sha1::new();
            
            if let Some(ph) = &prev_hash {
                hasher.update(ph);
            }
            hasher.update(&event.data);
            
            let computed_hash = hasher.finalize().to_vec();
            if computed_hash != event.hash {
                return false;
            }
            
            prev_hash = Some(event.hash.clone());
        }
        
        true
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let chain: StateChain = serde_json::from_str(&json)?;
        Ok(chain)
    }

    pub fn get_last_event(&self) -> Option<&ChainEvent> {
        self.events.last()
    }

    pub fn get_events(&self) -> &[ChainEvent] {
        &self.events
    }
}