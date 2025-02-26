# Theater Architecture

## Core Components

Theater is built around a WebAssembly-based actor system with several key components working together:

### TheaterRuntime

The `TheaterRuntime` is the central orchestrator responsible for:
- Managing all actor lifecycles (spawn, stop, restart)
- Routing messages between actors
- Maintaining parent-child supervision hierarchies
- Propagating events through the system

### ActorRuntime

Each actor is managed by an `ActorRuntime` that:
- Loads and initializes the WebAssembly component
- Sets up handlers based on manifest configuration
- Manages the actor's state chain
- Processes incoming messages and routes them to appropriate handlers

### ActorExecutor

The `ActorExecutor` provides the execution environment for WebAssembly actors:
- Executes WebAssembly functions with proper state management
- Ensures all state transitions are recorded in the hash chain
- Collects metrics on actor performance
- Handles cleanup during actor shutdown

### StateChain

The `StateChain` implements a verifiable history of all state transitions:
- Each state change is recorded as a chain event
- Events are cryptographically linked via SHA-1 hashing
- Complete history can be verified for tampering
- Supports persistence to disk and reloading

## Message Flow

```
Sender -> ActorMessage -> ActorRuntime -> ActorExecutor
    -> (Current State + Message) -> WebAssembly Handler
    -> (New State + Response) -> StateChain
    -> Response -> Sender
```

All messages flow through the actor runtime, which:
1. Receives messages via its mailbox
2. Routes to appropriate handler based on message type
3. Updates state chain with results
4. Returns responses to sender
5. Propagates events to parent actors when necessary

### Message Types

Theater supports several types of messages:

1. **Actor Requests** (synchronous):
   - Sender expects a response
   - Actor processes message and returns result
   - State changes are recorded in hash chain

2. **Actor Sends** (asynchronous):
   - Fire-and-forget messages
   - No response expected
   - State changes still recorded in hash chain

3. **HTTP Requests** (via HTTP server handler):
   - Incoming HTTP requests are converted to actor messages
   - Responses are converted back to HTTP responses
   - All interactions recorded in hash chain

4. **Management Commands**:
   - Control messages for actor lifecycle
   - Handled by TheaterRuntime
   - Include spawn, stop, restart operations

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
- Accessing children's state and event history

### Supervision Interface

The WIT interface for supervision enables parent actors to:
```
- spawn: Create new child actors
- list-children: List all child actor IDs
- stop-child: Stop a specific child actor
- restart-child: Restart a failed or stopped child
- get-child-state: Access the state of a child
- get-child-events: Retrieve the event history of a child
```

Each supervisor operation is handled by the Theater runtime, which:
1. Receives the command from the parent
2. Performs the requested operation
3. Updates both parent and child state as needed
4. Returns results or events to the parent

## State Management

Each state transition in Theater is verifiable and reproducible:

```
State0 (Initial) -> Hash0
Message1 + State0 -> State1 -> Hash1
Message2 + State1 -> State2 -> Hash2
```

The `StateChain` implementation ensures:
- Complete audit trail of all state changes
- Ability to replay any sequence of events
- Cross-machine verification of state
- SHA-1 based cryptographic linking of events

## WebAssembly Component Model

Theater leverages the WebAssembly Component Model for actors:
- Each actor is a WebAssembly component with defined interfaces
- Interfaces are specified using the WebAssembly Interface Type (WIT) format
- The host provides capabilities to components through well-defined interfaces
- Common types ensure interoperability between host and components

### Core WIT Interfaces

Theater defines several key interfaces in WIT:

1. **actor.wit**:
   ```wit
   interface actor {
       use types.{state};
       init: func(state: state, params: tuple<string>) -> result<tuple<state>, string>;
   }
   ```

2. **message-server.wit**:
   ```wit
   interface message-server-client {
       use types.{json, event};
       handle-send: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>>, string>;
       handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
   }
   ```

3. **supervisor.wit**:
   ```wit
   interface supervisor {
       spawn: func(manifest: string) -> result<string, string>;
       list-children: func() -> list<string>;
       stop-child: func(child-id: string) -> result<_, string>;
       restart-child: func(child-id: string) -> result<_, string>;
       get-child-state: func(child-id: string) -> result<list<u8>, string>;
       get-child-events: func(child-id: string) -> result<list<chain-event>, string>;
   }
   ```

## HTTP Integration

Theater provides built-in HTTP capabilities through:

1. **HTTP Server Handler**:
   - Actors can expose HTTP endpoints
   - HTTP requests convert to actor messages
   - Responses convert back to HTTP responses
   - All interactions recorded in state chain

2. **HTTP Client Interface**:
   - Actors can make HTTP requests to external services
   - Request/response pairs are recorded in state chain
   - Error handling follows actor model patterns

Configuration example:
```toml
[[handlers]]
type = "http-server"
config = { port = 8080 }
```

## Cross-Actor Communication

Actors communicate through:

1. **Message Server**:
   - Direct message passing between actors
   - Supports both request/response and one-way messages
   - All communications recorded in state chain
   - Message format is JSON (serialized to bytes)

2. **Supervision Tree**:
   - Parent-child communication
   - Lifecycle events and monitoring
   - Access to child state and events

## Management Interface

The `TheaterServer` provides an external management interface:
- TCP socket for command/control
- Actor lifecycle management (start/stop/list)
- Event subscription system
- Status monitoring

Commands include:
- `StartActor`: Spawn a new actor from manifest
- `StopActor`: Terminate a running actor
- `ListActors`: Enumerate all running actors
- `SubscribeToActor`: Receive event notifications from actor
- `UnsubscribeFromActor`: Stop receiving event notifications

## Design Philosophy

Theater's architecture follows several key principles:

1. **Verifiable State**
   - All state changes are recorded in hash chains
   - Complete history can be verified and replayed
   - Cryptographic linking prevents tampering

2. **Supervision Hierarchy**
   - Clear parent-child relationships
   - Robust error handling and recovery
   - Distributed responsibility

3. **Component-Based Design**
   - WebAssembly components with clean interfaces
   - Isolation through WebAssembly sandboxing
   - Interoperability through standard interface types

4. **Flexible Handlers**
   - Multiple handler types (message, HTTP, etc.)
   - Consistent interface patterns
   - Extensible for new capabilities
