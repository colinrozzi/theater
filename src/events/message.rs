use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageEventData {
    // Send message events
    SendMessageCall {
        recipient: String,
        message_type: String,
        size: usize,
    },
    SendMessageResult {
        recipient: String,
        success: bool,
    },

    // Request message events
    RequestMessageCall {
        recipient: String,
        message_type: String,
        size: usize,
    },
    RequestMessageResult {
        recipient: String,
        response_size: usize,
        success: bool,
    },

    // Handle received message events
    HandleMessageCall {
        sender: String,
        message_type: String,
        size: usize,
    },
    HandleMessageResult {
        sender: String,
        success: bool,
    },

    // Handle request events
    HandleRequestCall {
        sender: String,
        message_type: String,
        size: usize,
    },
    HandleRequestResult {
        sender: String,
        response_size: usize,
        success: bool,
    },

    // Error events
    Error {
        operation: String,
        recipient: Option<String>,
        message: String,
    },
}

pub struct MessageEvent {
    pub data: MessageEventData,
    pub timestamp: u64,
    pub description: Option<String>,
}
