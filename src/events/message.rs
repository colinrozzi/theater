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

    // Channel events
    OpenChannelCall {
        recipient: String,
        message_type: String,
        size: usize,
    },
    OpenChannelResult {
        recipient: String,
        channel_id: String,
        accepted: bool,
    },
    ChannelMessageCall {
        channel_id: String,
        msg: Vec<u8>,
    },
    ChannelMessageResult {
        channel_id: String,
        success: bool,
    },
    CloseChannelCall {
        channel_id: String,
    },
    CloseChannelResult {
        channel_id: String,
        success: bool,
    },
    HandleChannelOpenCall {
        sender: String,
        channel_id: String,
        message_type: String,
        size: usize,
    },
    HandleChannelOpenResult {
        sender: String,
        channel_id: String,
        accepted: bool,
    },
    HandleChannelMessageCall {
        channel_id: String,
        message_type: String,
        size: usize,
    },
    HandleChannelMessageResult {
        channel_id: String,
        success: bool,
    },
    HandleChannelCloseCall {
        channel_id: String,
    },
    HandleChannelCloseResult {
        channel_id: String,
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
