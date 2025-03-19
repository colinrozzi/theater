use serde::{Deserialize, Serialize};
use serde_json::json;

// Messages for starting stream production
#[derive(Serialize, Deserialize)]
struct StartStreamingRequest {
    consumer_id: String,
    items: usize,
    interval_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct StartStreamingResponse {
    channel_id: String,
    status: String,
}

// Messages sent over the channel
#[derive(Serialize, Deserialize)]
struct DataItem {
    sequence: usize,
    value: String,
    timestamp: u64,
}

// Channel messages
#[derive(Serialize, Deserialize)]
struct ChannelMessage {
    message_type: String,
    payload: serde_json::Value,
}

// Actor state
#[derive(Serialize, Deserialize, Default)]
struct ProducerState {
    active_channels: Vec<String>,
    streams_produced: usize,
}

// Handle incoming message requests
#[export_name = "ntwk:theater/message-server-client.handle-request"]
pub fn handle_request(state_json: Option<Vec<u8>>, params: Vec<u8>) -> Result<(Option<Vec<u8>>, Vec<u8>), String> {
    // Parse state
    let mut state: ProducerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ProducerState::default(),
    };
    
    // Parse request
    let request: serde_json::Value = serde_json::from_slice(&params).map_err(|e| e.to_string())?;
    
    // Handle different request types
    let response = match request["action"].as_str() {
        Some("start_streaming") => {
            // Parse specific request
            let streaming_request: StartStreamingRequest = serde_json::from_value(request["params"].clone())
                .map_err(|e| format!("Failed to parse streaming request: {}", e))?;
            
            // Open a channel to the consumer
            let channel_id = match open_channel_to_consumer(&streaming_request.consumer_id)? {
                Some(id) => id,
                None => return Err("Failed to open channel to consumer".to_string()),
            };
            
            // Save channel ID
            state.active_channels.push(channel_id.clone());
            state.streams_produced += 1;
            
            // Start sending data on the channel in the background
            // Note: In a real implementation, this would need to be done outside this handler
            // using scheduled tasks or a separate thread. This is just for demonstration.
            start_streaming(&channel_id, streaming_request.items, streaming_request.interval_ms)?;
            
            // Return success response
            json!({
                "status": "streaming_started",
                "channel_id": channel_id,
                "items": streaming_request.items,
            })
        },
        Some("status") => {
            json!({
                "active_channels": state.active_channels,
                "streams_produced": state.streams_produced,
            })
        },
        _ => {
            return Err(format!("Unknown action: {:?}", request["action"]));
        }
    };
    
    // Serialize the response
    let response_bytes = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
    
    // Serialize the updated state
    let updated_state = Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?);
    
    Ok((updated_state, response_bytes))
}

// Handle normal messages (non-request)
#[export_name = "ntwk:theater/message-server-client.handle-send"]
pub fn handle_send(_state_json: Option<Vec<u8>>, params: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
    // For this example, we don't need to handle fire-and-forget messages
    // But we could process them similar to the request handler
    println!("Received message: {}", String::from_utf8_lossy(&params));
    Ok(None) // No state change
}

// Handle channel open requests
#[export_name = "ntwk:theater/message-server-client.handle-channel-open"]
pub fn handle_channel_open(state_json: Option<Vec<u8>>, params: Vec<u8>) -> Result<(Option<Vec<u8>>, bool, Option<Vec<u8>>), String> {
    // Parse state
    let mut state: ProducerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ProducerState::default(),
    };
    
    // For this example, accept all channel open requests
    println!("Channel open request received: {}", String::from_utf8_lossy(&params));
    
    // Parse the initial message
    let message: serde_json::Value = match serde_json::from_slice(&params) {
        Ok(msg) => msg,
        Err(e) => {
            println!("Error parsing channel open message: {}", e);
            return Ok((Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?), true, None));
        }
    };
    
    // Prepare an optional initial response
    let initial_response = serde_json::to_vec(&json!({
        "message_type": "channel_accepted",
        "payload": {
            "producer_id": "streaming-producer",
            "message": "Welcome to the stream!"
        }
    })).ok();
    
    // Update state (in a real implementation, we'd track the channel)
    state.active_channels.push("incoming_channel".to_string());
    
    Ok((Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?), true, initial_response))
}

// Handle channel messages
#[export_name = "ntwk:theater/message-server-client.handle-channel-message"]
pub fn handle_channel_message(state_json: Option<Vec<u8>>, channel_id: String, msg: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
    // Parse state
    let state: ProducerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ProducerState::default(),
    };
    
    // Process the message
    println!("Received message on channel {}: {}", channel_id, String::from_utf8_lossy(&msg));
    
    // Parse the message
    let message: serde_json::Value = match serde_json::from_slice(&msg) {
        Ok(msg) => msg,
        Err(e) => {
            println!("Error parsing channel message: {}", e);
            return Ok(Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?));
        }
    };
    
    // Handle different message types (in a real implementation)
    if let Some(message_type) = message["message_type"].as_str() {
        match message_type {
            "status_request" => {
                // We might send back a status message on the channel here
                let status_message = json!({
                    "message_type": "status_response",
                    "payload": {
                        "active_channels": state.active_channels,
                        "streams_produced": state.streams_produced,
                    }
                });
                
                match send_on_channel(&channel_id, &status_message) {
                    Ok(_) => println!("Sent status response on channel {}", channel_id),
                    Err(e) => println!("Error sending status response: {}", e),
                }
            },
            _ => {
                println!("Unknown message type: {}", message_type);
            }
        }
    }
    
    // No state change in this example
    Ok(Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?))
}

// Handle channel close
#[export_name = "ntwk:theater/message-server-client.handle-channel-close"]
pub fn handle_channel_close(state_json: Option<Vec<u8>>, channel_id: String) -> Result<Option<Vec<u8>>, String> {
    // Parse state
    let mut state: ProducerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ProducerState::default(),
    };
    
    // Update state to remove the channel
    state.active_channels.retain(|id| id != &channel_id && id != "incoming_channel");
    
    println!("Channel {} closed", channel_id);
    
    Ok(Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?))
}

// Helper functions for messaging

// Open a channel to the consumer
fn open_channel_to_consumer(consumer_id: &str) -> Result<Option<String>, String> {
    // In this example we're simulating the async nature of channel opening
    // In production code, this would be an actual WebAssembly host function call
    
    // Initial message to send
    let initial_message = json!({
        "message_type": "stream_init",
        "payload": {
            "producer_id": "streaming-producer",
            "timestamp": chrono::Utc::now().timestamp_millis(),
        }
    });
    
    // Serialize the message
    let message_bytes = serde_json::to_vec(&initial_message).map_err(|e| e.to_string())?;
    
    // Call the host function (this is a simulated implementation)
    // In production, this would use the actual WebAssembly host function binding
    let channel_id = ntwk_theater_message_server_host_open_channel(consumer_id, &message_bytes)?;
    
    Ok(Some(channel_id))
}

// Send a message on an existing channel
fn send_on_channel(channel_id: &str, message: &serde_json::Value) -> Result<(), String> {
    // Serialize the message
    let message_bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    
    // Call the host function (this is a simulated implementation)
    // In production, this would use the actual WebAssembly host function binding
    ntwk_theater_message_server_host_send_on_channel(channel_id, &message_bytes)?;
    
    Ok(())
}

// Start streaming data on a channel
fn start_streaming(channel_id: &str, items: usize, _interval_ms: u64) -> Result<(), String> {
    // In a real implementation, this would set up a timer or separate thread
    // Here we'll just send a few items immediately for demonstration
    for i in 0..items.min(10) { // Limit to 10 for demo
        let data_item = DataItem {
            sequence: i,
            value: format!("Data item {}", i),
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };
        
        let channel_message = json!({
            "message_type": "data_item",
            "payload": data_item,
        });
        
        send_on_channel(channel_id, &channel_message)?;
        
        // In a real implementation, we'd wait for interval_ms between sends
        // std::thread::sleep(std::time::Duration::from_millis(interval_ms));
    }
    
    // Send a completion message
    let completion_message = json!({
        "message_type": "stream_complete",
        "payload": {
            "items_sent": items.min(10),
            "timestamp": chrono::Utc::now().timestamp_millis(),
        }
    });
    
    send_on_channel(channel_id, &completion_message)?;
    
    Ok(())
}

// Simulated host function bindings (these would be provided by the WebAssembly runtime)
// In a real implementation, these would be imports from the host environment

fn ntwk_theater_message_server_host_open_channel(actor_id: &str, msg: &[u8]) -> Result<String, String> {
    // This is just a placeholder for the WIT-generated binding
    println!("Opening channel to actor {}", actor_id);
    // Simulate a successful channel open
    Ok(format!("ch_{:x}", chrono::Utc::now().timestamp()))
}

fn ntwk_theater_message_server_host_send_on_channel(channel_id: &str, msg: &[u8]) -> Result<(), String> {
    // This is just a placeholder for the WIT-generated binding
    println!("Sending message on channel {}: {:?}", channel_id, msg);
    // Simulate a successful send
    Ok(())
}

fn ntwk_theater_message_server_host_close_channel(channel_id: &str) -> Result<(), String> {
    // This is just a placeholder for the WIT-generated binding
    println!("Closing channel {}", channel_id);
    // Simulate a successful close
    Ok(())
}
