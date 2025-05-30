# Theater Server Interface

This guide describes how to connect to and interact with the Theater server programmatically. The Theater server provides a TCP-based interface for managing actors, sending messages, and monitoring actor lifecycle events.

## Table of Contents

1. [Overview](#overview)
2. [Connecting to the Server](#connecting-to-the-server)
3. [Command and Response Protocol](#command-and-response-protocol)
4. [Managing Actor Lifecycle](#managing-actor-lifecycle)
5. [Messaging and Communication](#messaging-and-communication)
6. [Event Monitoring](#event-monitoring)
7. [Channel Communication](#channel-communication)
8. [Complete Example](#complete-example)

## Overview

The Theater server exposes a TCP interface that allows clients to:

- Start and stop actors
- Send messages to actors
- Monitor actor status and events
- Create communication channels between actors and external systems
- Query actor state and performance metrics

All communication with the server uses a simple length-delimited protocol with JSON-serialized commands and responses.

## Connecting to the Server

By default, the Theater server listens on `127.0.0.1:2823`. To connect to the server, you establish a TCP connection to this address.

### Using the TheaterClient

The simplest way to connect is using the `TheaterClient` provided in the Theater library:

```rust
use std::net::SocketAddr;
use theater::cli::client::TheaterClient;

async fn connect_to_theater() -> Result<TheaterClient, anyhow::Error> {
    let server_addr: SocketAddr = "127.0.0.1:2823".parse()?;
    let mut client = TheaterClient::new(server_addr);
    client.connect().await?;
    
    Ok(client)
}
```

### Manual Connection

If you're implementing a client in another language, you can connect using any TCP client:

1. Establish a TCP connection to the server address
2. Use a length-delimited frame format where each message is prefixed with its length as a 32-bit unsigned integer
3. Send and receive JSON-serialized commands and responses

Example in Python:

```python
import socket
import json
import struct

def connect_to_theater():
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.connect(("127.0.0.1", 2823))
    
    # Helper function to send a command
    def send_command(command):
        # Serialize command to JSON
        command_json = json.dumps(command).encode('utf-8')
        
        # Prepend length as a 32-bit unsigned integer in network byte order
        length_prefix = struct.pack('!I', len(command_json))
        
        # Send the length-prefixed message
        sock.sendall(length_prefix + command_json)
        
        # Receive the response length
        length_bytes = sock.recv(4)
        response_length = struct.unpack('!I', length_bytes)[0]
        
        # Receive the response
        response_json = sock.recv(response_length)
        return json.loads(response_json.decode('utf-8'))
    
    return sock, send_command
```

## Command and Response Protocol

### Commands

Commands to the Theater server follow this structure:

```json
{
  "type": "CommandType",
  "parameters": {
    // Command-specific parameters
  }
}
```

Where `type` is one of the `ManagementCommand` enum values, and `parameters` contains command-specific data.

### Responses

Responses from the Theater server follow this structure:

```json
{
  "type": "ResponseType",
  "data": {
    // Response-specific data
  }
}
```

Where `type` is one of the `ManagementResponse` enum values, and `data` contains response-specific information.

### Error Handling

If a command fails, the server responds with an `Error` response containing an error code and message:

```json
{
  "type": "Error",
  "error": {
    "code": "ErrorCode",
    "message": "Error message details"
  }
}
```

## Managing Actor Lifecycle

### Starting an Actor

To start a new actor, send a `StartActor` command with the actor's manifest content or path:

```rust
// Using TheaterClient
let manifest_path = "/path/to/manifest.toml";
client.start_actor(manifest_path.to_string(), None, false, false).await?;

// Manual Command
let command = ManagementCommand::StartActor {
    manifest: manifest_path.to_string(),
    initial_state: None,
    parent: false,
    subscribe: false,
};
let response = client.send_command(command).await?;
```

The `parent` parameter indicates whether the client wants to act as a supervisor for the actor.
The `subscribe` parameter indicates whether the client wants to receive events from the actor.

Response:

```json
{
  "ActorStarted": {
    "id": "act_12345abcde"
  }
}
```

### Stopping an Actor

To stop a running actor:

```rust
// Using TheaterClient
client.stop_actor(actor_id).await?;

// Manual Command
let command = ManagementCommand::StopActor { id: actor_id };
let response = client.send_command(command).await?;
```

Response:

```json
{
  "ActorStopped": {
    "id": "act_12345abcde"
  }
}
```

### Listing Actors

To get a list of all running actors:

```rust
// Using TheaterClient
let actors = client.list_actors().await?;

// Manual Command
let command = ManagementCommand::ListActors;
let response = client.send_command(command).await?;
```

Response:

```json
{
  "ActorList": {
    "actors": [
      ["act_12345abcde", "actor_name_1"],
      ["act_67890fghij", "actor_name_2"]
    ]
  }
}
```

### Restarting an Actor

To restart a failed or stopped actor:

```rust
// Using TheaterClient
client.restart_actor(actor_id).await?;

// Manual Command
let command = ManagementCommand::RestartActor { id: actor_id };
let response = client.send_command(command).await?;
```

Response:

```json
{
  "Restarted": {
    "id": "act_12345abcde"
  }
}
```

## Messaging and Communication

### Sending a Message to an Actor

To send a one-way message to an actor:

```rust
// Using TheaterClient
let message_data = b"Hello, actor!".to_vec();
client.send_actor_message(actor_id, message_data).await?;

// Manual Command
let command = ManagementCommand::SendActorMessage {
    id: actor_id,
    data: message_data,
};
let response = client.send_command(command).await?;
```

Response:

```json
{
  "SentMessage": {
    "id": "act_12345abcde"
  }
}
```

### Requesting a Response from an Actor

To send a message and receive a response:

```rust
// Using TheaterClient
let request_data = b"What is your status?".to_vec();
let response_data = client.request_actor_message(actor_id, request_data).await?;

// Manual Command
let command = ManagementCommand::RequestActorMessage {
    id: actor_id,
    data: request_data,
};
let response = client.send_command(command).await?;
```

Response:

```json
{
  "RequestedMessage": {
    "id": "act_12345abcde",
    "message": [115, 116, 97, 116, 117, 115, 32, 111, 107]
  }
}
```

## Event Monitoring

### Subscribing to Actor Events

To receive event notifications from an actor:

```rust
// Using TheaterClient
let subscription_id = client.subscribe_to_actor(actor_id).await?;

// Manual Command
let command = ManagementCommand::SubscribeToActor { id: actor_id };
let response = client.send_command(command).await?;
```

Response:

```json
{
  "Subscribed": {
    "id": "act_12345abcde",
    "subscription_id": "67890fghij"
  }
}
```

After subscribing, you'll receive `ActorEvent` messages whenever events occur in the actor:

```json
{
  "ActorEvent": {
    "event": {
      "id": "evt_12345",
      "actor_id": "act_12345abcde",
      "timestamp": 1620000000,
      "operation": "MessageReceived",
      "data": {...},
      "parent_hash": [...]
    }
  }
}
```

### Unsubscribing from Events

To stop receiving event notifications:

```rust
// Using TheaterClient
client.unsubscribe_from_actor(actor_id, subscription_id).await?;

// Manual Command
let command = ManagementCommand::UnsubscribeFromActor {
    id: actor_id,
    subscription_id,
};
let response = client.send_command(command).await?;
```

Response:

```json
{
  "Unsubscribed": {
    "id": "act_12345abcde"
  }
}
```

### Getting Actor Events History

To retrieve the event history of an actor:

```rust
// Using TheaterClient
let events = client.get_actor_events(actor_id).await?;

// Manual Command
let command = ManagementCommand::GetActorEvents { id: actor_id };
let response = client.send_command(command).await?;
```

Response:

```json
{
  "ActorEvents": {
    "id": "act_12345abcde",
    "events": [
      {
        "id": "evt_12345",
        "actor_id": "act_12345abcde",
        "timestamp": 1620000000,
        "operation": "ActorStarted",
        "data": {...},
        "parent_hash": null
      },
      // More events...
    ]
  }
}
```

## Channel Communication

Channels provide a bidirectional communication mechanism between external clients and actors.

### Opening a Channel

To open a new communication channel with an actor:

```rust
// Using TheaterClient
let initial_message = b"Hello, let's talk".to_vec();
let channel_id = client.open_channel(actor_id, initial_message).await?;

// Manual Command
let command = ManagementCommand::OpenChannel {
    actor_id: ChannelParticipant::Actor(actor_id),
    initial_message,
};
let response = client.send_command(command).await?;
```

Response:

```json
{
  "ChannelOpened": {
    "channel_id": "chan_12345abcde",
    "actor_id": {"Actor": "act_12345abcde"}
  }
}
```

### Sending a Message on a Channel

To send data through an established channel:

```rust
// Using TheaterClient
let message = b"Here's some data".to_vec();
client.send_on_channel(&channel_id, message).await?;

// Manual Command
let command = ManagementCommand::SendOnChannel {
    channel_id: channel_id.to_string(),
    message,
};
let response = client.send_command(command).await?;
```

Response:

```json
{
  "MessageSent": {
    "channel_id": "chan_12345abcde"
  }
}
```

### Receiving Messages on a Channel

After opening a channel, you can receive messages from the actor without sending a specific command:

```rust
// Using TheaterClient
loop {
    if let Some((channel_id, message)) = client.receive_channel_message().await? {
        println!("Received message on channel {}: {:?}", channel_id, message);
    }
}

// Manual approach - check for responses with ChannelMessage type