use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;

pub type State = Value;
pub type ActorOutput = Value;

pub trait Actor: Send {
    async fn init(&self) -> Result<Value>;
    async fn handle_event(&self, state: State, event: Event) -> Result<(State, Event)>;
    async fn verify_state(&self, state: &Value) -> bool;
}
