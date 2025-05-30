# Data Flow

This page explains how data moves through the Theater system, from external inputs to actor processing and eventual outputs.

## External Input Processing

### HTTP Requests

For HTTP-based interactions:

1. HTTP request arrives at Theater Server
2. Server parses request and identifies target actor
3. Request is converted to a message
4. Message is recorded in Event Chain
5. Message is queued for delivery to actor

### CLI Commands

For CLI-initiated actions:

1. User issues command via CLI
2. CLI connects to Theater Server
3. Command is converted to management message
4. Message is recorded in Event Chain
5. Server processes management command

### Message Delivery

For actor-to-actor messaging:

1. Sender actor invokes messaging API
2. Message is recorded in Event Chain
3. Message is queued for delivery
4. Target actor receives message

## Actor Processing

### State Retrieval

When actors access state:

1. Actor invokes Store API
2. Runtime validates access request
3. State request is recorded in Event Chain
4. Store retrieves state data
5. Data is returned to actor

### Computation

During actor computation:

1. Actor processes message data
2. Actor may access state via Store
3. Actor may use handlers for external services
4. All handler invocations are recorded in Event Chain

### State Updates

When actors modify state:

1. Actor creates new state data
2. Actor invokes Store API to save state
3. Update request is recorded in Event Chain
4. Store validates and persists new state
5. Store returns new state reference to actor

## External Output Processing

### HTTP Responses

For HTTP response generation:

1. Actor produces response data
2. Response is recorded in Event Chain
3. Response is converted to HTTP format
4. HTTP response is sent to client

### Event Streaming

For event subscription:

1. Client subscribes to events via Theater Server
2. New events are recorded in Event Chain
3. Server filters events based on subscription
4. Matching events are streamed to client

## Special Data Flows

### Supervision

During supervision operations:

1. Runtime detects actor failure
2. Failure is recorded in Event Chain
3. Supervisor actor is notified
4. Supervisor decides on action
5. Supervisory action is recorded in Event Chain
6. Runtime implements supervisory action

### Replay

During event replay:

1. Replay request identifies starting point
2. Events are retrieved from Event Chain
3. Actor state is initialized
4. Events are applied sequentially
5. Actor reaches target state

Understanding these data flows helps visualize how information moves through the Theater system and how different components collaborate to process data securely and deterministically.
