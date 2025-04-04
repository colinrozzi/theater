# Message Server Handler

The Message Server Handler is the primary mechanism for actor-to-actor communication in Theater. It enables actors to send messages to each other, establish request-response patterns, and create persistent communication channels.

## Overview

The Message Server Handler implements two key interfaces:

1. **message-server-host**: Functions that actors can call to send messages to other actors
2. **message-server-client**: Functions that actors implement to receive and process messages

Together, these interfaces enable a complete messaging system within the Theater ecosystem.

## Configuration

The Message Server Handler requires minimal configuration in the actor's manifest:

```toml
[[handlers]]
type = "message-server"
config = {}
```

The handler is automatically added to all actors, so you don't need to explicitly include it in your manifest.

## Messaging Patterns

The Message Server Handler supports three primary communication patterns:

### 1. One-Way Messages (Send)

Send messages are "fire-and-forget" - the sender doesn't wait for a response.

**Host Interface (actor calling)**:
```wit
send: func(actor-id: actor-id, msg: json) -> result<_, string>;
```

**Client Interface (actor implementing)**:
```wit
handle-send: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>>, string>;
```

**Usage Example**:
```rust
// Sending a message
message_server_host::send(target_id, message_data)?;

// Handling a message
fn handle_send(state: Option<Vec<u8>>, params: (Vec<u8>,)) -> Result<(Option<Vec<u8>>,), String> {
    // Process message and update state
    Ok((new_state,))
}
```

### 2. Request-Response Messages

Request messages expect a response from the recipient.

**Host Interface (actor calling)**:
```wit
request: func(actor-id: actor-id, msg: json) -> result<json, string>;
```

**Client Interface (actor implementing)**:
```wit
handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
```

**Usage Example**:
```rust
// Sending a request
let response = message_server_host::request(target_id, request_data)?;

// Handling a request
fn handle_request(state: Option<Vec<u8>>, params: (Vec<u8>,)) -> Result<(Option<Vec<u8>>, (Vec<u8>,)), String> {
    // Process request, update state, and prepare response
    Ok((new_state, (response,)))
}
```

### 3. Channel-Based Communication

Channels provide a persistent communication pathway between actors.

**Host Interface (actor calling)**:
```wit
open-channel: func(actor-id: actor-id, initial-msg: json) -> result<string, string>;
send-on-channel: func(channel-id: string, msg: json) -> result<_, string>;
close-channel: func(channel-id: string) -> result<_, string>;
```

**Client Interface (actor implementing)**:
```wit
handle-channel-open: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<channel-accept>>, string>;
handle-channel-message: func(state: option<json>, params: tuple<string, json>) -> result<tuple<option<json>>, string>;
handle-channel-close: func(state: option<json>, params: tuple<string>) -> result<tuple<option<json>>, string>;
```

**Usage Example**:
```rust
// Opening a channel
let channel_id = message_server_host::open_channel(target_id, initial_message)?;

// Sending on a channel
message_server_host::send_on_channel(channel_id, message_data)?;

// Closing a channel
message_server_host::close_channel(channel_id)?;

// Handling channel operations
fn handle_channel_open(state: Option<Vec<u8>>, params: (Vec<u8>,)) -> Result<(Option<Vec<u8>>, (ChannelAccept,)), String> {
    // Process channel open request and decide whether to accept
    Ok((new_state, (channel_accept,)))
}
```

## Message Format

Messages are typically serialized as JSON bytes with a standard structure:

```json
{
  "type": "message_type",
  "action": "specific_action",
  "payload": {
    "key1": "value1",
    "key2": 42
  },
  "metadata": {
    "timestamp": "2025-03-20T12:34:56Z",
    "message_id": "msg-12345"
  }
}
```

## State Management

Every message operation is recorded in the actor's state chain, maintaining a verifiable history of all communications. The event data includes:

- Message type (send, request, channel)
- Timestamp
- Recipient information
- Success/failure status
- Message size

## Error Handling

The Message Server Handler provides detailed error information for various failure scenarios:

1. **Invalid Actor ID**: When the target actor doesn't exist
2. **Delivery Failure**: When the message can't be delivered
3. **Processing Error**: When the target actor fails to process the message
4. **Channel Errors**: When channel operations fail (non-existent channel, closed channel)

All errors are properly recorded in the state chain for debugging.

## Implementation Details

The Message Server Handler processes messages through a dedicated task that:

1. Receives incoming messages from the actor's mailbox
2. Processes messages based on their type
3. Calls the appropriate actor function
4. Updates the actor's state
5. Returns responses (for request messages)
6. Records all operations in the state chain

## Channel Management

Channels have additional lifecycle management:

1. **Channel Creation**: A unique channel ID is generated for each channel
2. **Channel Acceptance**: The target actor must explicitly accept the channel
3. **Channel State**: The handler tracks which channels are open
4. **Channel Closure**: Either participant can close the channel

## Best Practices

1. **Message Structure**: Use consistent message structures with clear type and action fields
2. **Error Handling**: Always handle potential errors from message operations
3. **Channel Management**: Close channels when they're no longer needed
4. **State Design**: Keep message handlers focused on state transitions
5. **Timeout Handling**: Consider implementing timeouts for request operations

## Security Considerations

1. **Message Validation**: Always validate incoming messages before processing
2. **Actor ID Verification**: Verify actor IDs before sending messages
3. **Payload Size**: Be mindful of message payload sizes
4. **Error Exposure**: Don't expose sensitive information in error messages

## Examples

### Example 1: Simple Request-Response

```rust
// Actor sending a request
pub fn get_data_from_actor(target_id: &str) -> Result<Data, String> {
    let request = serde_json::json!({
        "type": "request",
        "action": "get_data",
        "payload": {}
    });
    
    let request_bytes = serde_json::to_vec(&request).unwrap();
    let response_bytes = message_server_host::request(target_id, request_bytes)?;
    let response: Data = serde_json::from_slice(&response_bytes).unwrap();
    
    Ok(response)
}

// Actor handling the request
fn handle_request(state: Option<Vec<u8>>, params: (Vec<u8>,)) -> Result<(Option<Vec<u8>>, (Vec<u8>,)), String> {
    let message: serde_json::Value = serde_json::from_slice(&params.0).unwrap();
    
    if message["action"] == "get_data" {
        let data = serde_json::to_vec(&get_data()).unwrap();
        Ok((state, (data,)))
    } else {
        Err("Unknown action".to_string())
    }
}
```

### Example 2: Channel Communication

```rust
// Actor opening a channel
pub fn open_data_stream(target_id: &str) -> Result<String, String> {
    let initial_message = serde_json::json!({
        "type": "channel",
        "action": "open_data_stream",
        "payload": {
            "frequency": "1s"
        }
    });
    
    let message_bytes = serde_json::to_vec(&initial_message).unwrap();
    let channel_id = message_server_host::open_channel(target_id, message_bytes)?;
    
    Ok(channel_id)
}

// Actor handling channel open
fn handle_channel_open(state: Option<Vec<u8>>, params: (Vec<u8>,)) -> Result<(Option<Vec<u8>>, (ChannelAccept,)), String> {
    let message: serde_json::Value = serde_json::from_slice(&params.0).unwrap();
    
    if message["action"] == "open_data_stream" {
        // Accept the channel
        let channel_accept = ChannelAccept {
            accepted: true,
            message: None,
        };
        
        Ok((state, (channel_accept,)))
    } else {
        // Reject the channel
        let channel_accept = ChannelAccept {
            accepted: false,
            message: Some(b"Unsupported action".to_vec()),
        };
        
        Ok((state, (channel_accept,)))
    }
}
```

## Related Topics

- [HTTP Framework Handler](http-framework.md) - For HTTP-based communication
- [Supervisor Handler](supervisor.md) - For parent-child communication
- [State Management](../core-concepts/state-management.md) - For understanding state chain integration
