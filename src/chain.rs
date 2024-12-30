use crate::wasm::Event;
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

pub struct ChainRequest {
    pub request_type: ChainRequestType,
    pub response_tx: oneshot::Sender<ChainResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainResponse {
    Head(Option<String>),
    ChainEntry(Option<ChainEntry>),
    FullChain(Vec<(String, ChainEntry)>),
}

#[derive(Debug)]
pub enum ChainRequestType {
    GetHead,
    GetChainEntry(String),
    GetChain,
    AddEvent { event: Event },
}

pub struct ChainRequestHandler {
    chain: HashChain,
    chain_rx: mpsc::Receiver<ChainRequest>,
}

impl ChainRequestHandler {
    pub fn new(chain_rx: mpsc::Receiver<ChainRequest>) -> Self {
        let chain = HashChain::new();
        Self { chain, chain_rx }
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(req) = self.chain_rx.recv() => {
                    self.handle_chain_request(req).await;
                }
                else => {
                    info!("Chain request handler shutting down");
                    break;
                }
            }
        }
    }

    pub async fn handle_chain_request(&mut self, req: ChainRequest) {
        let response = match req.request_type {
            ChainRequestType::GetHead => {
                let head = self.chain.get_head().map(|h| h.to_string());
                ChainResponse::Head(head)
            }
            ChainRequestType::GetChainEntry(hash) => {
                let entry = self.chain.get_chain_entry(&hash);
                ChainResponse::ChainEntry(entry.cloned())
            }
            ChainRequestType::GetChain => {
                let full_chain = self.chain.get_full_chain();
                ChainResponse::FullChain(full_chain)
            }
            ChainRequestType::AddEvent { event } => {
                let hash = self.chain.add(event);
                ChainResponse::Head(Some(hash))
            }
        };

        let _ = req.response_tx.send(response);
    }
}
