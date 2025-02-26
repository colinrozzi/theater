use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// Base trait for all chain events
pub trait ChainEventData:
    Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static
{
    /// The event type identifier
    fn event_type(&self) -> &'static str;

    /// Human-readable description of the event
    fn description(&self) -> String;

    /// Convert to JSON
    fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

// Import specific event modules
pub mod filesystem;
pub mod http;
pub mod message;
pub mod runtime;
pub mod supervisor;
