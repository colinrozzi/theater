# Theater Architecture

## Core Components

Each actor in Theater is a WebAssembly component that follows a simple but powerful pattern:
- Accepts JSON messages and state
- Returns new state and response messages
- Creates hash chain entries for state transitions

This uniformity enables powerful composition patterns while keeping the system comprehensible. By using JSON for both messages and state, Theater avoids complex serialization issues and makes debugging straightforward.

## Message Flow

```
Sender -> JSON Message -> Actor
Actor -> (Current State + Message) -> Handler
Handler -> (New State + Response) -> Hash Chain Entry
Hash Chain Entry -> Response -> Sender
```

### Example Flow

Consider a simple counter actor:

```json
// Initial state
{
  "count": 0
}

// Incoming message
{
  "type": "increment",
  "amount": 5
}

// New state
{
  "count": 5
}

// Response message
{
  "type": "increment_complete",
  "new_count": 5
}
```

Each transition creates a hash chain entry containing:
- The previous state hash
- The incoming message
- The new state
- A timestamp

## Supervision Tree

```
Parent Actor
├── Child Actor 1 (gets lifecycle events)
│   └── Grandchild Actor
└── Child Actor 2 (gets lifecycle events)
```

Parent actors serve as supervisors with several key responsibilities:
- Spawning child actors with specific configurations
- Receiving lifecycle notifications (started, stopped, crashed)
- Managing child restarts and shutdowns
- Verifying children's state transitions
- Handling escalated errors

### Lifecycle Management

When a parent spawns a child actor:
1. Parent receives the child's actor ID and management interface
2. Child startup is verified through the hash chain
3. Parent begins receiving lifecycle events
4. Parent can monitor child state transitions

If a child crashes:
1. Parent receives crash notification with error details
2. Parent can inspect child's state history through hash chain
3. Parent decides whether to restart child with same or modified state
4. All decisions are recorded in parent's own hash chain

## State Management

Each state transition in Theater is verifiable and reproducible:

```
State0 (Initial) -> Hash0
Message1 + State0 -> State1 -> Hash1
Message2 + State1 -> State2 -> Hash2
```

The hash chain ensures:
- Complete audit trail of all state changes
- Ability to replay any sequence of events
- Cross-machine verification of state
- Deterministic debugging of issues

## HTTP Integration

Theater provides built-in HTTP capabilities:
- Actors can expose HTTP endpoints
- HTTP requests/responses are transformed into actor messages
- State changes from HTTP interactions are recorded in hash chain
- Multiple actors can share HTTP interfaces

Example HTTP handler configuration:
```toml
[[handlers]]
type = "Http"
config = { port = 8080 }

[[handlers]]
type = "Http-server"
config = { port = 8081 }
```

## Cross-Actor Communication

Actors communicate through:
1. Direct message passing (same host)
2. HTTP endpoints (remote actors)
3. Custom interface implementations

All communications:
- Use the JSON message format
- Create hash chain entries
- Can be verified and replayed
- Maintain parent-child relationships

## Design Philosophy

Theater's architecture follows several key principles:

1. **Simplicity Over Complexity**
   - One message format (JSON)
   - One state format (JSON)
   - One primary interface pattern

2. **Verifiable Everything**
   - All state changes create hash chain entries
   - All parent-child relationships are tracked
   - All communications can be replayed

3. **Debuggability First**
   - Complete state history available
   - Deterministic replay of any sequence
   - Clear parent-child relationships
   - Structured error handling

4. **Flexible Composition**
   - Actors can implement multiple interfaces
   - HTTP integration built-in
   - Extensible handler system

This architecture enables Theater to provide robust actor-based systems while maintaining clarity and debuggability - key features that are often missing in distributed systems.