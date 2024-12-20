use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type State = Value;
pub type ActorOutput = Value;

/// The content of an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// The type of event that occurred
    #[serde(rename = "type")]
    pub type_: String,
    /// The data associated with this event
    pub data: Value,
}

pub trait Actor: Send {
    fn init(&self) -> Result<Value>;
    fn handle_event(&self, state: State, event: Event) -> Result<(State, Event)>;
    fn verify_state(&self, state: &Value) -> bool;
}
