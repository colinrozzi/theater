use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorInput {
    Message(Value),
    HttpRequest {
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorOutput {
    Message(Value),
    HttpResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    },
}

#[derive(Debug)]
pub enum MessageMetadata {
    ActorSource {
        source_actor: String,
        source_chain_state: String,
    },
    HttpRequest {
        response_channel: oneshot::Sender<ActorOutput>,
    },
}

#[derive(Debug)]
pub struct ActorMessage {
    pub content: ActorInput,
    pub metadata: Option<MessageMetadata>,
}

pub trait Actor: Send {
    fn init(&self) -> Result<Value>;
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;
    fn verify_state(&self, state: &Value) -> bool;
}
