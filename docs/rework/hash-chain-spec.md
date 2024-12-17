# Hash Chain Specification

## Overview

The hash chain is the foundation of Theater's state verification system. It provides an immutable, verifiable record of all state transitions and events for each actor.

## Chain Structure

### Chain Elements

Each element in the chain consists of:

```rust
struct ChainElement {
    // The actual event data
    event: Event,
    
    // Hash of the previous element (None for genesis)
    previous_hash: Option<Hash>,
    
    // Hash of this element (computed)
    hash: Hash,
}
```

### Event Structure

Each event captures a single state transition:

```rust
struct Event {
    // Type of event that occurred
    event_type: EventType,

    // Data specific to the event type
    data: EventData,
}
```

### Event Types

```rust
enum EventType {
    // Initial state creation
    Genesis,
    
    // State change from input
    StateTransition,
    
    // Incoming message from another actor
    MessageReceived {
        from: ActorId,
        message_hash: Hash,
    },
    
    // Outgoing message to another actor
    MessageSent {
        to: ActorId,
        message_hash: Hash,
    },
    
    // Custom event type (extensible)
    Custom(String),
}
```

### Event Data

```rust
enum EventData {
    // State transition data
    StateTransition(StateTransitionData),
    
    // Message data
    Message(MessageData),
    
    // Custom event data (extensible)
    Custom(HashMap<String, String>),
}
```

## Hashing

### Hash Computation

1. Each element's hash is computed over:
   - The complete event data
   - The previous element's hash
   - The sequence number

2. Hash algorithm requirements:
   - Must be cryptographically secure
   - Must be deterministic
   - Recommended: Blake3 (fast, secure, designed for tree hashing)

### Example Hash Computation:

```rust
fn compute_hash(element: &ChainElement) -> Hash {
    let mut hasher = Blake3::new();
    
    // Hash the previous hash if it exists
    if let Some(prev) = &element.previous_hash {
        hasher.update(prev.as_bytes());
    }
    
    // Hash the sequence number
    hasher.update(&element.sequence.to_le_bytes());
    
    // Hash the event data
    hasher.update(&serialize(&element.event));
    
    Hash::from(hasher.finalize())
}
```

## Verification

### Single Element Verification

To verify a single element:
1. Compute the hash of the element
2. Verify it matches the stored hash
3. Verify sequence number is correct
4. Verify previous hash matches previous element

### Chain Verification

To verify the entire chain:
1. Start from genesis element (sequence = 0)
2. Verify each element sequentially
3. Ensure no gaps in sequence numbers
4. Verify state hashes match actual states

```rust
fn verify_chain(chain: &[ChainElement]) -> Result<bool> {
    let mut previous: Option<&ChainElement> = None;
    
    for element in chain {
        // Verify sequence
        if let Some(prev) = previous {
            if element.sequence != prev.sequence + 1 {
                return Err(Error::SequenceGap);
            }
        } else if element.sequence != 0 {
            return Err(Error::InvalidGenesis);
        }
        
        // Verify hash links
        if element.previous_hash != previous.map(|p| p.hash) {
            return Err(Error::BrokenChain);
        }
        
        // Verify element hash
        if compute_hash(element) != element.hash {
            return Err(Error::InvalidHash);
        }
        
        previous = Some(element);
    }
    
    Ok(true)
}
```

## State Verification

### State Hashing

1. States are hashed independently of events
2. State hash is computed over complete state data
3. State hashes are included in events for verification

```rust
fn compute_state_hash(state: &ActorState) -> Hash {
    let mut hasher = Blake3::new();
    hasher.update(&serialize(state));
    Hash::from(hasher.finalize())
}
```

### State Reproduction

To reproduce an actor's state:
1. Start from genesis state
2. Replay all events sequentially
3. Verify each resulting state hash
4. Final state must match chain head

## Chain Storage

### Requirements

1. **Append-Only**: Chain must be strictly append-only
2. **Durability**: Chain data must be persistent
3. **Sequential Access**: Efficient sequential reading
4. **Random Access**: Index-based element lookup
5. **Atomic Updates**: Chain updates must be atomic

### Recommended Structure

```rust
struct ChainStorage {
    // Memory-mapped file for chain data
    data_file: MmapFile,
    
    // Index for random access
    index: BTreeMap<u64, FileOffset>,
    
    // Latest chain head
    head: ChainElement,
}
```

## Inter-Actor Verification

### Message Verification

When actors communicate:
1. Sender includes its chain head hash
2. Receiver can verify sender's state
3. Both actors record message in their chains
4. Cross-chain verification becomes possible

### Cross-Chain Verification

```rust
fn verify_cross_chain(
    sender_chain: &[ChainElement],
    receiver_chain: &[ChainElement],
    message: &Message,
) -> Result<bool> {
    // Find send event in sender's chain
    let send_event = find_send_event(sender_chain, message)?;
    
    // Find receive event in receiver's chain
    let receive_event = find_receive_event(receiver_chain, message)?;
    
    // Verify message hashes match
    if send_event.message_hash != receive_event.message_hash {
        return Err(Error::MessageMismatch);
    }
    
    // Verify temporal ordering
    if send_event.timestamp >= receive_event.timestamp {
        return Err(Error::TimeOrderViolation);
    }
    
    Ok(true)
}
```

## Performance Considerations

1. **Chain Growth**
   - Chains grow linearly with activity
   - Consider pruning old events
   - Use efficient storage formats
   
2. **Verification Cost**
   - Cache verified chain segments
   - Parallelize verification when possible
   - Use incremental verification

3. **Storage Efficiency**
   - Compress old chain segments
   - Index key points for faster verification
   - Consider hierarchical storage

## Security Considerations

1. **Hash Algorithm**
   - Must be cryptographically secure
   - Should be quantum-resistant
   - Must have strong collision resistance

2. **Chain Protection**
   - Prevent unauthorized modifications
   - Detect tampering attempts
   - Secure storage access

3. **State Protection**
   - Verify all state transitions
   - Protect state storage
   - Validate state hashes
