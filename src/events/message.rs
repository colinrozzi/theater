use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageEventData {
    MessageReceived {
        sender: String,
        message_type: String,
        size: usize,
    },
    MessageSent {
        recipient: String,
        message_type: String,
        size: usize,
    },
    Error {
        message: String,
        context: Option<String>,
    },
}

pub struct MessageEvent {
    pub data: MessageEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
