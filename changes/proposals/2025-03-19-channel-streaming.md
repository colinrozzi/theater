# Channel Streaming Capability for Message Server

| Field     | Value                                                           |
|-----------|-----------------------------------------------------------------|
| Date      | 2025-03-19                                                      |
| Author    | Theater Team                                                    |
| Status    | In Progress                                                     |
| Priority  | High                                                            |

## Overview

Add a new "channel" communication pattern to the existing message server to support bidirectional streaming between actors. This will enable long-standing connections where actors can continuously send and receive data, ideal for scenarios requiring ongoing data exchange.

## Motivation

The current message server supports:
1. `send`: One-way message sending (fire and forget)
2. `request`: Request-response pattern (blocking call)

However, there's no efficient way to handle streaming data patterns where:
- Multiple messages are exchanged over a single logical connection
- Data flows continuously in both directions
- Back-pressure needs to be managed
- The connection has a lifecycle (open, active, closed)

Channels enable important use cases like:
- Stream processing of large datasets
- Real-time event subscriptions
- Continuous sensor data monitoring
- Interactive sessions between actors

## Implementation Status

### 1. Completed ✅
- Added channel types to `types.wit`
- Added channel operations to `message-server.wit`
- Added channel-related message types in `messages.rs`
- Implemented channel ID generation and tracking
- Added channel event types for tracing all operations
- Updated the `MessageServerHost` to track active channels
- Implemented channel handler functions in the host
- Added channel handler functions registration
- Added channel message processing

### 2. In Progress 🚧
- Integration testing with existing Theater commands
- Channel timeouts and lifecycle management
- Back-pressure handling implementation
- Channel metrics collection

### 3. Upcoming ⏱️
- CLI commands for working with channels
- Channel discovery API
- Timeout settings configuration
- Rate limiting capabilities

## Example Implementation

A complete example showing how to use channels has been implemented in `/examples/channel-streaming/`. It includes:

1. A producer actor that initiates a stream and sends data items
2. A consumer actor that processes the stream data
3. Documentation on using the channel system

The example demonstrates:
- Channel establishment and negotiation
- Bidirectional data exchange
- Acknowledgment patterns
- Proper error handling
- Channel lifecycle management

## Next Steps

1. Complete integration testing
2. Implement channel timeouts and lifecycle management
3. Add channel-related CLI commands
4. Update documentation with best practices
5. Conduct performance testing for high-throughput scenarios

## Technical Details

Refer to the original proposal for detailed technical specifications and the example implementation for usage patterns.
