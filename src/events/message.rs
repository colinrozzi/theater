use super::ChainEventData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageEvent {
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

impl ChainEventData for MessageEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::MessageReceived { .. } => "message.received",
            Self::MessageSent { .. } => "message.sent",
            Self::Error { .. } => "message.error",
        }
    }
    
    fn description(&self) -> String {
        match self {
            Self::MessageReceived { sender, message_type, size } => {
                format!("Received {} message from {} ({} bytes)", 
                    message_type, sender, size)
            },
            Self::MessageSent { recipient, message_type, size } => {
                format!("Sent {} message to {} ({} bytes)", 
                    message_type, recipient, size)
            },
            Self::Error { message, context } => {
                if let Some(ctx) = context {
                    format!("Message error in {}: {}", ctx, message)
                } else {
                    format!("Message error: {}", message)
                }
            },
        }
    }
}
