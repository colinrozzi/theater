use serde::{Deserialize, Serialize};
use serde_json::json;

// Messages for starting stream consumption
#[derive(Serialize, Deserialize)]
struct IncomingStreamRequest {
    producer_id: String,
    items: usize,
    interval_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct IncomingStreamResponse {
    status: String,
}

// Data structures
#[derive(Serialize, Deserialize, Debug)]
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
struct ConsumerState {
    active_channels: Vec<String>,
    total_items_received: usize,
    current_streams: usize,
    completed_streams: usize,
}

// Handle incoming message requests
#[export_name = "ntwk:theater/message-server-client.handle-request"]
pub fn handle_request(state_json: Option<Vec<u8>>, params: Vec<u8>) -> Result<(Option<Vec<u8>>, Vec<u8>), String> {
    // Parse state
    let mut state: ConsumerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ConsumerState::default(),
    };
    
    // Parse request
    let request: serde_json::Value = serde_json::from_slice(&params).map_err(|e| e.to_string())?;
    
    // Handle different request types
    let response = match request["action"].as_str() {
        Some("request_stream") => {
            // Parse specific request
            let stream_request: IncomingStreamRequest = serde_json::from_value(request["params"].clone())
                .map_err(|e| format!("Failed to parse stream request: {}", e))?;
            
            // Request the producer to start streaming
            match request_stream_from_producer(&stream_request) {
                Ok(channel_id) => {
                    // Save channel ID
                    state.active_channels.push(channel_id.clone());
                    state.current_streams += 1;
                    
                    json!({
                        "status": "stream_requested",
                        "channel_id": channel_id,
                        "items": stream_request.items,
                    })
                },
                Err(e) => {
                    json!({
                        "status": "error",
                        "message": format!("Failed to request stream: {}", e)
                    })
                }
            }
        },
        Some("status") => {
            json!({
                "active_channels": state.active_channels,
                "total_items_received": state.total_items_received,
                "current_streams": state.current_streams,
                "completed_streams": state.completed_streams,
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
    println!("Received message: {}", String::from_utf8_lossy(&params));
    Ok(None) // No state change
}

// Handle channel open requests
#[export_name = "ntwk:theater/message-server-client.handle-channel-open"]
pub fn handle_channel_open(state_json: Option<Vec<u8>>, params: Vec<u8>) -> Result<(Option<Vec<u8>>, bool, Option<Vec<u8>>), String> {
    // Parse state
    let mut state: ConsumerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ConsumerState::default(),
    };
    
    println!("Channel open request received: {}", String::from_utf8_lossy(&params));
    
    // Parse the initial message
    let message: serde_json::Value = match serde_json::from_slice(&params) {
        Ok(msg) => msg,
        Err(e) => {
            println!("Error parsing channel open message: {}", e);
            return Ok((Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?), false, None));
        }
    };
    
    // Check the message type
    let message_type = message["message_type"].as_str().unwrap_or("unknown");
    
    // Accept the channel if it's a stream initialization
    if message_type == "stream_init" {
        // Update the state
        state.active_channels.push("incoming_stream".to_string());
        state.current_streams += 1;
        
        // Prepare an optional initial response
        let initial_response = serde_json::to_vec(&json!({
            "message_type": "stream_ready",
            "payload": {
                "consumer_id": "streaming-consumer",
                "message": "Ready to receive data"
            }
        })).ok();
        
        println!("Accepted stream channel");
        return Ok((Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?), true, initial_response));
    } else {
        println!("Rejected channel - unknown message type: {}", message_type);
        return Ok((Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?), false, None));
    }
}

// Handle channel messages
#[export_name = "ntwk:theater/message-server-client.handle-channel-message"]
pub fn handle_channel_message(state_json: Option<Vec<u8>>, channel_id: String, msg: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
    // Parse state
    let mut state: ConsumerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ConsumerState::default(),
    };
    
    println!("Received message on channel {}", channel_id);
    
    // Parse the message
    let message: serde_json::Value = match serde_json::from_slice(&msg) {
        Ok(msg) => msg,
        Err(e) => {
            println!("Error parsing channel message: {}", e);
            return Ok(Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?));
        }
    };
    
    // Handle different message types
    if let Some(message_type) = message["message_type"].as_str() {
        match message_type {
            "data_item" => {
                // Parse the data item
                if let Ok(data_item) = serde_json::from_value::<DataItem>(message["payload"].clone()) {
                    println!("Received data item: {:?}", data_item);
                    state.total_items_received += 1;
                    
                    // Send an acknowledgment
                    let ack_message = json!({
                        "message_type": "item_ack",
                        "payload": {
                            "sequence": data_item.sequence,
                            "timestamp": chrono::Utc::now().timestamp_millis(),
                        }
                    });
                    
                    match send_on_channel(&channel_id, &ack_message) {
                        Ok(_) => println!("Sent acknowledgment for item {}", data_item.sequence),
                        Err(e) => println!("Error sending acknowledgment: {}", e),
                    }
                }
            },
            "stream_complete" => {
                println!("Stream complete notification received");
                
                // Update state
                state.current_streams -= 1;
                state.completed_streams += 1;
                state.active_channels.retain(|id| id != &channel_id && id != "incoming_stream");
                
                // Send a completion acknowledgment
                let completion_ack = json!({
                    "message_type": "stream_complete_ack",
                    "payload": {
                        "consumer_id": "streaming-consumer",
                        "items_received": state.total_items_received,
                        "timestamp": chrono::Utc::now().timestamp_millis(),
                    }
                });
                
                match send_on_channel(&channel_id, &completion_ack) {
                    Ok(_) => println!("Sent stream completion acknowledgment"),
                    Err(e) => println!("Error sending completion acknowledgment: {}", e),
                }
                
                // Close the channel after all is done
                match close_channel(&channel_id) {
                    Ok(_) => println!("Closed channel {}", channel_id),
                    Err(e) => println!("Error closing channel: {}", e),
                }
            },
            _ => {
                println!("Unknown message type: {}", message_type);
            }
        }
    }
    
    Ok(Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?))
}

// Handle channel close
#[export_name = "ntwk:theater/message-server-client.handle-channel-close"]
pub fn handle_channel_close(state_json: Option<Vec<u8>>, channel_id: String) -> Result<Option<Vec<u8>>, String> {
    // Parse state
    let mut state: ConsumerState = match state_json {
        Some(json) => serde_json::from_slice(&json).map_err(|e| e.to_string())?,
        None => ConsumerState::default(),
    };
    
    // Update state to remove the channel
    if state.active_channels.iter().any(|id| id == &channel_id || id == "incoming_stream") {
        state.current_streams -= 1;
    }
    
    state.active_channels.retain(|id| id != &channel_id && id != "incoming_stream");
    
    println!("Channel {} closed", channel_id);
    
    Ok(Some(serde_json::to_vec(&state).map_err(|e| e.to_string())?))
}

// Helper functions for messaging

// Request a stream from a producer
fn request_stream_from_producer(request: &IncomingStreamRequest) -> Result<String, String> {
    // Create message to send to the producer
    let message = json!({
        "action": "start_streaming",
        "params": {
            "consumer_id": "streaming-consumer", // Our ID
            "items": request.items,
            "interval_ms": request.interval_ms
        }
    });
    
    // Serialize the message
    let message_bytes = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
    
    // Call the host function to send the request
    // In production, this would use the actual WebAssembly host function binding
    let response_bytes = ntwk_theater_message_server_host_request(&request.producer_id, &message_bytes)?;
    
    // Parse the response
    let response: serde_json::Value = serde_json::from_slice(&response_bytes).map_err(|e| e.to_string())?;
    
    // Extract the channel ID
    let channel_id = response["channel_id"].as_str()
        .ok_or_else(|| "Channel ID not found in response".to_string())?
        .to_string();
    
    Ok(channel_id)
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

// Close a channel
fn close_channel(channel_id: &str) -> Result<(), String> {
    // Call the host function (this is a simulated implementation)
    // In production, this would use the actual WebAssembly host function binding
    ntwk_theater_message_server_host_close_channel(channel_id)?;
    
    Ok(())
}

// Simulated host function bindings (these would be provided by the WebAssembly runtime)
// In a real implementation, these would be imports from the host environment

fn ntwk_theater_message_server_host_request(actor_id: &str, msg: &[u8]) -> Result<Vec<u8>, String> {
    // This is just a placeholder for the WIT-generated binding
    println!("Sending request to actor {}", actor_id);
    // Simulate a successful response with a channel ID
    let response = json!({
        "status": "streaming_started",
        "channel_id": format!("ch_{:x}", chrono::Utc::now().timestamp()),
        "items": 10,
    });
    
    serde_json::to_vec(&response).map_err(|e| e.to_string())
}

fn ntwk_theater_message_server_host_send_on_channel(channel_id: &str, msg: &[u8]) -> Result<(), String> {
    // This is just a placeholder for the WIT-generated binding
    println!("Sending message on channel {}", channel_id);
    // Simulate a successful send
    Ok(())
}

fn ntwk_theater_message_server_host_close_channel(channel_id: &str) -> Result<(), String> {
    // This is just a placeholder for the WIT-generated binding
    println!("Closing channel {}", channel_id);
    // Simulate a successful close
    Ok(())
}
