# State Management & Hash Chains

Theater's state management system provides cryptographic verification of all state transitions while maintaining simplicity and debuggability.

## Core Concepts

### Hash Chain Structure

```
State0 (Initial) ─────> Hash0
       ↓
Message1 + State0 ───> State1 ─────> Hash1
                         ↓
Message2 + State1 ───> State2 ─────> Hash2
```

Each hash chain entry contains:
- Parent hash reference
- Current state snapshot
- Message that triggered transition
- Timestamp
- Actor metadata

### JSON State Format

All state in Theater is JSON, making it:
- Human readable
- Easy to debug
- Simple to serialize
- Language agnostic

Example state evolution:

```json
// Initial State (Hash0)
{
  "count": 0,
  "last_updated": "2025-01-14T10:00:00Z"
}

// Message1
{
  "type": "increment",
  "amount": 5
}

// State1 (Hash1)
{
  "count": 5,
  "last_updated": "2025-01-14T10:00:01Z"
}

// Message2
{
  "type": "multiply",
  "factor": 2
}

// State2 (Hash2)
{
  "count": 10,
  "last_updated": "2025-01-14T10:00:02Z"
}
```

## Verification Process

Any state can be verified through:

1. Hash Chain Validation:
```json
{
  "hash": "hash2",
  "parent_hash": "hash1",
  "state": {
    "count": 10,
    "last_updated": "2025-01-14T10:00:02Z"
  },
  "message": {
    "type": "multiply",
    "factor": 2
  },
  "timestamp": "2025-01-14T10:00:02Z",
  "actor_id": "counter-123"
}
```

2. State Replay:
- Start with initial state
- Apply each message in sequence
- Verify generated hashes match
- Compare final state

3. Cross-Machine Verification:
- Share hash chain entries
- Independently replay state transitions
- Compare hash results
- Verify state consistency

## State Transitions

### Basic Transition
```json
{
  "type": "state_transition",
  "old_state_hash": "hash1",
  "message": {
    "type": "increment",
    "amount": 5
  },
  "new_state": {
    "count": 15,
    "last_updated": "2025-01-14T10:00:03Z"
  },
  "new_state_hash": "hash3"
}
```

### Compound State Changes
Multiple changes can be atomic:
```json
{
  "type": "compound_transition",
  "old_state_hash": "hash3",
  "messages": [
    {
      "type": "increment",
      "amount": 5
    },
    {
      "type": "multiply",
      "factor": 2
    }
  ],
  "new_state": {
    "count": 40,
    "last_updated": "2025-01-14T10:00:04Z"
  },
  "new_state_hash": "hash4"
}
```

## State History

Theater maintains complete state history:

1. Direct Access:
```json
{
  "type": "state_request",
  "hash": "hash2",
  "response": {
    "state": {
      "count": 10,
      "last_updated": "2025-01-14T10:00:02Z"
    },
    "metadata": {
      "actor_id": "counter-123",
      "timestamp": "2025-01-14T10:00:02Z"
    }
  }
}
```

2. History Traversal:
```json
{
  "type": "history_request",
  "start_hash": "hash4",
  "end_hash": "hash2",
  "transitions": [
    {
      "hash": "hash4",
      "parent": "hash3",
      "state": { /* ... */ }
    },
    {
      "hash": "hash3",
      "parent": "hash2",
      "state": { /* ... */ }
    }
  ]
}
```

## Advanced Features

### Time Travel Debugging
```json
{
  "type": "debug_request",
  "target_hash": "hash2",
  "replay": true,
  "breakpoints": [
    {
      "condition": "state.count > 8",
      "actions": ["pause", "log"]
    }
  ]
}
```

### State Merkle Proofs
Prove properties about state history:
```json
{
  "type": "proof_request",
  "property": "count_never_negative",
  "start_hash": "hash0",
  "end_hash": "hash4",
  "proof": {
    "type": "merkle",
    "root": "hash4",
    "path": [ /* merkle proof elements */ ],
    "verified": true
  }
}
```

### Cross-Actor State Verification
```json
{
  "type": "cross_verify",
  "actors": ["counter-123", "counter-456"],
  "property": "counts_match",
  "timestamp": "2025-01-14T10:00:04Z",
  "verified": true
}
```

## Best Practices

1. **State Design**
   - Keep state minimal
   - Use flat structures when possible
   - Include timestamps for ordering
   - Consider queryability

2. **Message Design**
   - Make messages self-contained
   - Include operation type
   - Add context for debugging
   - Consider idempotency

3. **Hash Chain Management**
   - Regular verification
   - Prune old history when safe
   - Backup important states
   - Monitor chain growth

4. **Debugging Strategy**
   - Use time travel debugging
   - Verify state at key points
   - Track state dependencies
   - Monitor transition patterns

## Performance Considerations

1. **State Size**
   - Keep states compact
   - Consider partial updates
   - Use appropriate data structures
   - Monitor growth over time

2. **Hash Chain Growth**
   - Implement pruning strategies
   - Archive old states
   - Monitor chain length
   - Consider state snapshots

3. **Verification Overhead**
   - Batch verifications
   - Cache frequent states
   - Use incremental verification
   - Balance frequency vs security