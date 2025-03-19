# Channel Streaming Example

This example demonstrates the bidirectional streaming capability in Theater using channels. It consists of two actors:

1. **Producer Actor**: Initiates and sends a stream of data items
2. **Consumer Actor**: Receives and processes the data stream

## Overview

The channel streaming feature enables long-standing connections between actors where data can be continuously exchanged in both directions. This is particularly useful for:

- Stream processing of large datasets
- Real-time event subscriptions
- Continuous sensor data monitoring
- Interactive sessions between actors

## Running the Example

### 1. Build the actors

```bash
cd examples/channel-streaming
theater build producer.rs --output producer.wasm
theater build consumer.rs --output consumer.wasm
```

### 2. Start the Theater server

```bash
theater server
```

### 3. Start the consumer actor

```bash
theater start consumer.toml
```

This will output the actor ID for the consumer, which we'll need in the next step.

### 4. Start the producer actor

```bash
theater start producer.toml
```

### 5. Request a stream from the producer

Send a message to the producer to initiate streaming to the consumer:

```bash
theater message <producer-id> '{
  "action": "start_streaming",
  "params": {
    "consumer_id": "<consumer-id>",
    "items": 10,
    "interval_ms": 500
  }
}'
```

Replace `<producer-id>` and `<consumer-id>` with the actual IDs from the previous steps.

### 6. View the logs

In a separate terminal, you can watch the logs to see the actors exchanging messages over the channel:

```bash
theater logs <producer-id>
```

And in yet another terminal:

```bash
theater logs <consumer-id>
```

## How It Works

1. **Channel Establishment**:
   - The producer opens a channel to the consumer using `open-channel`
   - The consumer accepts the channel connection in `handle-channel-open`
   - Both sides can exchange initial setup information

2. **Data Streaming**:
   - The producer sends data items on the channel using `send-on-channel`
   - The consumer processes each item and sends acknowledgments back
   - Each message is tracked in the event history

3. **Channel Termination**:
   - Either side can close the channel using `close-channel`
   - The other side is notified via `handle-channel-close`

## Message Flow Diagram

```
Producer                   Consumer
   |                          |
   |---- open-channel ------->|
   |                          |
   |<--- channel accepted ----|
   |                          |
   |---- data_item(1) ------->|
   |<--- ack(1) --------------|
   |                          |
   |---- data_item(2) ------->|
   |<--- ack(2) --------------|
   |         ...              |
   |                          |
   |---- stream_complete ---->|
   |<--- complete_ack --------|
   |                          |
   |---- close-channel ------>|
   |                          |
```

## Application to Real-World Scenarios

This pattern can be applied to many practical use cases:

- **Database Query Streaming**: Stream large result sets without loading everything into memory
- **Real-time Data Processing**: Process sensor data or events as they occur
- **File Transfers**: Transfer large files in manageable chunks with progress tracking
- **Collaborative Applications**: Maintain ongoing connections between user sessions
- **Pub/Sub Systems**: Create event subscription systems with acknowledgments

## Error Handling

The example includes various error handling patterns:

- Handling channel establishment failures
- Managing unexpected message formats
- Dealing with premature channel closures
- Tracking and reporting errors via the event system
