# Change Request: Actor Chain Linking

## Overview
Enable basic chain linking between communicating actors for verifiable state tracking.

## Implementation
1. Include chain state in messages:
```rust
pub struct ActorMessage {
    content: ActorInput,
    source_actor: String,
    source_chain_state: String, // Hash of sender's chain head
}
```

2. Record message events in chains:
```rust
pub enum ChainEvent {
    // ... existing events ...
    MessageReceived {
        source_actor: String,
        source_chain_state: String, // Sender's chain state
        message: Value,
    },
    MessageSent {
        target_actor: String,
        our_chain_state: String,   // Our chain state at send time
        message: Value,
    }
}
```

Each actor simply records what it knows at the time - when it sends a message or receives one. This creates a simple but verifiable trail of inter-actor communication.

## Questions
- How do we verify chain integrity across reboots?
- Should we persist chains to disk?