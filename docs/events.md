# Event Structure

## Overview
Theater uses a simple but powerful event structure to track all state changes and interactions within the system. Each event represents a single occurrence in the system and is connected to its predecessor, forming a chain that captures the complete history of an actor's state.

## Basic Structure
Every event in Theater follows this structure:

```json
{
  "event": {
    "type": "type-of-event",
    "data": "event-specific-data"
  },
  "parent": "hash-of-parent-event"
}
```

### Fields Explained
- `event.type`: Identifies what kind of event occurred
- `event.data`: Contains all relevant information about the event
- `parent`: Hash of the previous event in the chain, establishing the event ordering

## Common Event Types

### State Change
```json
{
  "event": {
    "type": "state_change",
    "data": {
      "new_state": "the-new-state-value"
    }
  },
  "parent": "previous-event-hash"
}
```

### Message Received
```json
{
  "event": {
    "type": "message_received",
    "data": {
      "from": "sender-id",
      "content": "message-content"
    }
  },
  "parent": "previous-event-hash"
}
```

### HTTP Request
```json
{
  "event": {
    "type": "http_request",
    "data": {
      "method": "GET",
      "path": "/example",
      "headers": {},
      "body": "request-body"
    }
  },
  "parent": "previous-event-hash"
}
```

## Design Philosophy
The event structure is intentionally minimal, capturing only the essential information needed to understand what happened and maintain the chain of events. This simplicity makes the system easier to understand, implement, and extend.

Additional metadata (timestamps, actor IDs, verification hashes, etc.) can be added in the future if needed, but the core structure should remain as simple as possible.