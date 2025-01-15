# Supervision in Theater

Theater's supervision model builds on traditional actor supervision patterns while adding cryptographic verification and complete state history awareness.

## Core Concepts

### Supervisor Responsibilities

A supervisor in Theater:
- Spawns child actors
- Monitors child lifecycle events
- Verifies child state transitions
- Handles child errors
- Manages restarts
- Records all supervision actions in its hash chain

### State Verification

```
Parent Hash Chain               Child Hash Chain
     |                              |
  State0 -----------------------> State0 (spawn)
     |                              |
  State1 <----------------------- State1 (update)
     |                              |
  State2 -----------------------> State2 (restart)
     |                              |
```

Every child state transition:
1. Creates a hash chain entry
2. Is reported to parent
3. Gets verified against parent's chain
4. Becomes part of supervision record

## Lifecycle Events

### Actor Startup
```json
{
  "type": "actor_started",
  "actor_id": "child-123",
  "initial_state": {
    "count": 0
  },
  "state_hash": "hash0"
}
```

### State Updates
```json
{
  "type": "state_updated",
  "actor_id": "child-123",
  "old_state_hash": "hash0",
  "new_state_hash": "hash1",
  "transition_message": {
    "type": "increment",
    "amount": 5
  }
}
```

### Actor Termination
```json
{
  "type": "actor_terminated",
  "actor_id": "child-123",
  "final_state_hash": "hash5",
  "reason": "shutdown"
}
```

## Error Handling

When a child actor encounters an error:

1. Error is captured with context:
```json
{
  "type": "actor_error",
  "actor_id": "child-123",
  "error": "division by zero",
  "state_hash": "hash7",
  "message": {
    "type": "divide",
    "value": 10,
    "by": 0
  }
}
```

2. Supervisor decides response:
- Restart actor
- Stop actor
- Escalate error
- Ignore error

3. Decision is recorded:
```json
{
  "type": "supervision_action",
  "actor_id": "child-123",
  "action": "restart",
  "error_hash": "error7",
  "new_state_hash": "hash8"
}
```

## Restart Strategies

Theater supports several restart strategies:

### Reset to Initial
```json
{
  "strategy": "reset",
  "initial_state": {
    "count": 0
  }
}
```

### Resume from Last Good
```json
{
  "strategy": "resume",
  "state_hash": "hash6"
}
```

### Custom State Recovery
```json
{
  "strategy": "custom",
  "state": {
    "count": 5,
    "last_error": "hash7",
    "retries": 1
  }
}
```

Each restart:
1. Creates new hash chain entry
2. Links to previous state history
3. Records restart reason
4. Maintains verification chain

## Supervision Tree Patterns

### Basic Supervisor
```
Supervisor
├── Worker1
└── Worker2
```
- Manages independent workers
- Each worker has own state
- Simple restart strategies

### Hierarchical Supervision
```
SupervisorA
├── SupervisorB
│   ├── Worker1
│   └── Worker2
└── SupervisorC
    └── Worker3
```
- Nested supervision
- Error escalation path
- State verification at each level

### State-Linked Supervision
```
StatefulSupervisor
├── DependentWorker1
└── DependentWorker2
```
- Workers share state context
- Coordinated restarts
- Verified state relationships

## HTTP Handler Supervision

HTTP handlers are supervised like any other actor:
- Handler crashes create error events
- State transitions are verified
- Restarts maintain request context
- Hash chain records all requests

Example handler error:
```json
{
  "type": "handler_error",
  "handler": "http",
  "port": 8080,
  "request": {
    "method": "POST",
    "path": "/api/v1/count"
  },
  "error": "internal_error",
  "state_hash": "hash12"
}
```

## Verification and Debugging

Supervision provides powerful debugging capabilities:
1. Complete error context
2. State history before error
3. Message that caused error
4. All restart attempts
5. Final resolution

This enables:
- Exact error reproduction
- State verification at time of error
- Restart strategy validation
- System-wide state consistency checks

## Best Practices

1. **Error Classification**
   - Distinguish between expected and unexpected errors
   - Consider error frequency in restart strategies
   - Track error patterns across restarts

2. **State Management**
   - Verify state after restarts
   - Keep restart state minimal
   - Consider state dependencies

3. **Supervision Hierarchy**
   - Group related actors
   - Isolate failure domains
   - Plan error escalation paths

4. **Monitoring**
   - Track restart frequencies
   - Monitor state transition patterns
   - Alert on supervision anomalies