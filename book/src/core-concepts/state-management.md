# State Management & Hash Chains

Theater's state management system provides cryptographic verification of all state transitions while maintaining simplicity and debuggability. This document describes the current implementation and planned enhancements.

## Current Implementation

The `StateChain` struct in `chain/mod.rs` forms the foundation of the state management system, providing:

1. **Event Chain Structure**:
   - Each event is cryptographically linked to its predecessor
   - SHA-1 is used as the hashing algorithm
   - Every event contains:
     - Hash: SHA-1 hash of the event
     - Parent Hash: Reference to previous event (or None for initial event)
     - Event Type: String identifier for the operation
     - Data: Serialized event data as bytes
     - Timestamp: When the event occurred

2. **Chain Verification**:
   - The `verify()` method validates the entire chain integrity
   - Each event hash is recomputed and compared to stored hash
   - Any tampering with past events will break the chain

3. **Core Operations**:
   - `add_event()`: Record new events in the chain
   - `get_last_event()`: Retrieve the most recent event
   - `get_events()`: Access the complete event history
   - `save_to_file()`: Persist chain to disk
   - `load_from_file()`: Restore chain from disk

### StateChain Structure

In code, the `StateChain` implementation looks like:

```rust
pub struct StateChain {
    events: Vec<ChainEvent>,
    current_hash: Option<Vec<u8>>,
}

pub struct ChainEvent {
    pub hash: Vec<u8>,
    pub parent_hash: Option<Vec<u8>>,
    pub event_type: String,
    pub data: Vec<u8>,
    pub timestamp: u64,
}
```

The `ChainEvent` struct directly maps to the WIT interface definition, ensuring compatibility between host and WebAssembly components.

### Hash Chain Visualization

```
Event0 (Initial) ─────> Hash0 = SHA1(Data0)
       ↓
Event1 + Hash0 ───────> Hash1 = SHA1(Hash0 + Data1)
                         ↓
Event2 + Hash1 ───────> Hash2 = SHA1(Hash1 + Data2)
```

Each event's hash is computed using:
1. The previous event's hash (if any)
2. The current event's data

This creates a tamper-evident chain where modifying any past event would invalidate all subsequent hashes.

### Actor State Management

For actors, state is managed through a cycle of:

1. **State Storage**:
   - Actor instance stores state as optional bytes
   - State is typically serialized JSON or other format
   - State can be empty (None) for stateless actors

2. **Function Execution**:
   - Current state is passed to WebAssembly function
   - Function processes input and returns new state
   - State updates are recorded in hash chain

3. **State Retrieval**:
   - Latest state can be accessed from actor
   - Complete event history is available for inspection
   - Parent actors can access child state via supervision interface

Example from `actor_executor.rs`:

```rust
// Execute the call with current state
let (new_state, results) = self.actor_instance.call_function(&name, state, params).await?;

// Update stored state
self.actor_instance.store.data_mut().set_state(new_state);
```

## Working with State

### Adding Events

To add a new event to the chain:

```rust
// Create event and update chain
let event = chain.add_event("increment".to_string(), increment_data);
```

This automatically:
1. Computes the new hash based on previous state
2. Records timestamp and event metadata
3. Adds the event to the chain
4. Updates the current hash pointer

### Verifying Chain Integrity

The entire chain can be verified with:

```rust
// Check if chain is valid
let is_valid = chain.verify();
```

This recalculates all hashes and ensures:
1. Each event's stored hash matches its computed hash
2. Parent hash references form an unbroken chain
3. No events have been tampered with or reordered

### Accessing Event History

Complete event history is available:

```rust
// Get all events
let events = chain.get_events();

// Get most recent event
let last_event = chain.get_last_event();
```

This enables:
- Inspection of state transitions
- Debugging of state-related issues
- Verification of specific events

### Persistence

Chains can be saved and loaded using the [Store System](store/README.md) for content-addressable storage:

```rust
// Save chain to disk
chain.save_to_file(path)?;

// Load chain from disk
let loaded_chain = StateChain::load_from_file(path)?;
```

This facilitates:
- Persistence across restarts
- Chain migration between systems
- Backup and restore operations

## Integration with Actor Model

The state chain is deeply integrated with Theater's actor model:

1. **Actor Initialization**:
   - Initial state is empty or loaded from storage
   - First event is "init" with initial parameters
   - Chain is established with actor creation

2. **Message Handling**:
   - Each message creates a new event
   - State transitions are recorded in chain
   - Responses include new state reference

3. **Supervision**:
   - Parent actors can access child state chains
   - State verification occurs during supervision
   - Chain events propagate through supervision tree

4. **Actor Shutdown**:
   - Final state is recorded in chain
   - Chain may be persisted for later use
   - Completed chains can be archived or analyzed

## WebAssembly Interface

The WIT interface for chains looks like:

```wit
record chain-event {
    hash: list<u8>,
    parent-hash: option<list<u8>>,
    event-type: string,
    data: list<u8>,
    timestamp: u64
}
```

This allows WebAssembly components to:
- Receive and process chain events
- Create properly formatted events
- Maintain compatibility with host chain implementation

## Best Practices

1. **State Design**
   - Keep state minimal and focused
   - Use structured formats (JSON, MessagePack, etc.)
   - Include metadata for easier debugging
   - Consider versioning for schema evolution

2. **Event Types**
   - Use descriptive event type names
   - Follow consistent naming patterns
   - Document event type meanings
   - Consider event type registries for complex systems

3. **Chain Management**
   - Verify chains regularly
   - Monitor chain growth
   - Implement appropriate retention policies
   - Backup important chains

4. **Performance Considerations**
   - Monitor chain length in long-running actors
   - Consider state snapshots for large histories
   - Profile hash computation overhead
   - Optimize serialization formats

## Planned Enhancements

Future versions of Theater will expand the state management system with:

1. **Advanced Verification**:
   - Merkle tree-based verification for efficient proofs
   - Cross-actor state verification protocols
   - State invariant checking and enforcement

2. **Debugging Tools**:
   - Time travel debugging through chain traversal
   - Event visualization and analysis
   - Anomaly detection in state transitions
   - Comparison tools for chain divergence

3. **Performance Optimizations**:
   - Chain pruning and compression
   - Partial chain verification
   - Optimized hash algorithms
   - Incremental state updates

4. **Distribution Features**:
   - Chain replication across nodes
   - Consensus protocols for shared chains
   - Partial chain synchronization
   - Federated chain verification
