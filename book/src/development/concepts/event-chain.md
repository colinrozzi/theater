# Event Chain System

## Overview

The Event Chain system in Theater is a fundamental mechanism for recording and verifying all interactions between actors and the outside world. The chain serves as a complete, cryptographically-linked audit trail of an actor's entire lifecycle and all its external interactions.

## Purpose and Benefits

The Event Chain provides several critical capabilities:

1. **Complete Interaction Record**: Every message, HTTP request, file operation, and other external interaction is recorded in the chain.

2. **Deterministic Replay**: Because WebAssembly execution is deterministic and all external inputs are recorded, any actor's execution can be precisely replayed on any system.

3. **Cryptographic Verification**: Each event in the chain is linked to its predecessors via SHA-1 hashing, creating a tamper-evident record of all activity.

4. **Debugging and Inspection**: The complete event history allows for detailed debugging and analysis of actor behavior.

5. **Failure Recovery**: The chain enables recovering from failures by replaying events up to a known good state.

## Chain Event Structure

Each event in the chain is structured as follows:

```rust
pub struct ChainEvent {
    pub hash: Vec<u8>,               // Cryptographic hash of this event
    pub parent_hash: Option<Vec<u8>>, // Hash of the previous event (None for first event)
    pub event_type: String,          // Type identifier for the event
    pub data: Vec<u8>,               // Event-specific data payload
    pub timestamp: u64,              // Event timestamp
    pub description: Option<String>, // Optional human-readable description
}
```

## Event Types

The chain records various types of events, each capturing a different kind of interaction:

1. **WASM Events**: Function calls into and out of the WebAssembly actor
2. **HTTP Events**: Incoming requests and outgoing responses via HTTP handlers
3. **Message Events**: Inter-actor communications through the message system
4. **Filesystem Events**: File operations performed by the actor
5. **Store Events**: Interactions with the content-addressable storage
6. **Supervisor Events**: Parent-child actor lifecycle events
7. **Runtime Events**: Actor lifecycle events (start, stop, restart)
8. **Timing Events**: Timer and scheduling operations

## Chain Formation Process

1. When an actor performs an action that interacts with the outside world, a `ChainEventData` structure is created.
2. This structure includes the event type, specific event data, timestamp, and optional description.
3. The event is converted to a `ChainEvent` and linked to the previous event via its parent hash.
4. A cryptographic hash of the event is computed and stored, forming the link in the chain.
5. The new event becomes the head of the chain, and its hash becomes the parent hash for the next event.

## Verification

The chain's integrity can be verified at any time by:

1. Starting with the first event (parent_hash = None)
2. For each subsequent event:
   - Computing the hash of the previous event and the current event's data
   - Comparing with the stored hash value

If any event has been tampered with, the hash verification will fail, invalidating the chain from that point forward.

## Use Cases

### Actor Migration
The event chain enables seamless migration of actors between systems:
- Serialize the chain and transfer it to the new system
- Reconstruct the actor by replaying the chain
- Continue execution with the complete history intact

### Failure Recovery
When an actor crashes:
- Examine the chain to identify the cause of failure
- Restart the actor and replay events up to a previous good state
- Continue execution from the known good state

### Security Auditing
The immutable event history provides:
- Complete audit trails for security analysis
- Evidence of potential tampering or unauthorized access
- Proof of execution for critical operations

## Implementation Details

The chain implementation ensures that:
1. All calls to WebAssembly actors pass through the `ActorExecutor`
2. External effects (HTTP, filesystem, etc.) are recorded before execution
3. State changes are included in the chain as they occur
4. No actor operation can bypass the chain recording process

## Persistence

The chain can be:
- Kept in memory during execution
- Written to disk for long-term storage
- Exported as JSON for analysis or transfer
- Imported from storage to resume operation

## Future Extensions

The chain system is designed to be extensible for future capabilities:
- Integration with distributed ledger technologies
- Advanced cryptographic verification schemes
- Performance optimizations for high-throughput systems
