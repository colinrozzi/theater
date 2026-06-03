//! The concrete `ChainEvent` type Theater uses for its actor event chains.
//!
//! `ChainEvent` is intentionally not a `theater_chain::Event<D>` — it
//! carries its payload as opaque CGRF bytes (`data: Vec<u8>`) so the
//! runtime can hash/serialize uniformly without needing to know the
//! concrete payload type at compile time.

use std::fmt;

use serde::{Deserialize, Serialize};
use wasmtime::component::{ComponentType, Lift, Lower};

use crate::event::EventType;

/// A single immutable event in an actor's execution history.
///
/// Each event carries the hash of its parent, forming a cryptographically
/// linked chain. The runtime computes the hash and broadcasts the event;
/// retention is the subscriber's responsibility.
#[derive(Debug, Clone, Serialize, Deserialize, ComponentType, Lift, Lower, Eq)]
#[component(record)]
pub struct ChainEvent {
    /// Content hash of this event (over `parent_hash`, `event_type`, `data`).
    pub hash: Vec<u8>,
    /// Hash of the parent event, or `None` for the first event in the chain.
    #[component(name = "parent-hash")]
    pub parent_hash: Option<Vec<u8>>,
    /// Type identifier for the event (e.g. `"state_change"`, `"http_request"`).
    #[component(name = "event-type")]
    pub event_type: String,
    /// Event payload, serialized.
    pub data: Vec<u8>,
}

impl fmt::Display for ChainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hash_str = hex::encode(&self.hash);
        let short_hash = if hash_str.len() > 7 {
            &hash_str[0..7]
        } else {
            &hash_str
        };

        let parent_str = match &self.parent_hash {
            Some(ph) => {
                let ph_str = hex::encode(ph);
                if ph_str.len() > 7 {
                    format!("(parent: {}...)", &ph_str[0..7])
                } else {
                    format!("(parent: {})", ph_str)
                }
            }
            None => "(root)".to_string(),
        };

        let content = if let Ok(value) = packr::abi::decode(&self.data) {
            format!("{}", value)
        } else if let Ok(text) = std::str::from_utf8(&self.data) {
            let preview = if text.len() > 80 {
                format!("{}...", &text[0..77])
            } else {
                text.to_string()
            };
            format!("'{}'", preview)
        } else {
            format!("{} bytes", self.data.len())
        };

        write!(
            f,
            "Event[{}] {} {} {}",
            short_hash,
            parent_str,
            console::style(&self.event_type).cyan(),
            content
        )
    }
}

impl EventType for ChainEvent {
    fn event_type(&self) -> String {
        self.event_type.clone()
    }

    fn len(&self) -> usize {
        self.data.len()
    }
}

impl PartialEq for ChainEvent {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl std::hash::Hash for ChainEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}
