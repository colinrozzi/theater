# System Architecture

Theater's architecture is designed around the principles of isolation, determinism, and traceability. This page provides a high-level overview of how the system components interact to deliver these guarantees.

## System Components

### Theater Runtime

The Theater Runtime is the core orchestration layer that:
- Manages actor lifecycle (creation, execution, termination)
- Implements the supervision system
- Coordinates message delivery between actors
- Maintains the event chain
- Provides access to the store system

### Actor Executor

The Actor Executor is responsible for:
- Loading WebAssembly components
- Instantiating component instances
- Providing host functions to components
- Managing component memory and resources
- Executing component functions in response to messages

### Event Chain System

The Event Chain tracks:
- All inputs to actors
- All outputs from actors
- State changes
- Error conditions
- Supervision actions

Every action in the system is recorded in a verifiable chain of events that enables:
- Deterministic replay
- Auditing
- Debugging
- System verification

### Store System

The content-addressable Store provides:
- Persistent storage for actor state
- Version control for state changes
- Efficient storage through content-addressing
- Verification of stored content

### Handler System

Handlers extend actor functionality by providing:
- Access to system services
- Integration with external systems
- Standard capabilities (HTTP, filesystem, etc.)
- Custom functionality through a plugin architecture

## Data Flow

1. **Input Processing**:
   - External requests enter through the Theater Server
   - Requests are converted to messages
   - Messages are recorded in the Event Chain
   - Messages are delivered to target actors

2. **Actor Execution**:
   - Actor Executor loads actor component
   - Message handlers are invoked
   - Actor may access state via the Store
   - Actor may use handlers to access services

3. **Output Handling**:
   - Actor responses are recorded in the Event Chain
   - Responses are delivered to requesters
   - State changes are persisted to the Store

## Design Principles

Theater's architecture is built on several key design principles:

1. **Isolation through WebAssembly**: 
   - Actors run in sandboxed environments
   - Component model provides capability-based security

2. **Explicit State Management**:
   - All state is explicitly managed through the Store
   - No hidden or shared state between actors

3. **Explicit Communication**:
   - All communication happens through messages
   - No direct actor-to-actor function calls

4. **Comprehensive Tracing**:
   - All system actions are recorded
   - Chain provides cryptographic verification

5. **Hierarchical Supervision**:
   - Actors are arranged in supervision trees
   - Parent actors manage child lifecycle

These principles work together to create a system that is secure, deterministic, and verifiable, making it ideal for applications where these properties are critical.
