# Component Relationships

This page details how the various components of Theater interact with each other, providing a deeper understanding of the system's internal architecture.

## Core Components

### Theater Runtime

The Theater Runtime is the central orchestration component that manages the entire system:

- **Relationship with Actor Executor**: The Runtime uses the Actor Executor to instantiate and run actors
- **Relationship with Store**: The Runtime coordinates with the Store for state persistence
- **Relationship with Event Chain**: The Runtime records all system events in the Chain

### Actor Executor

The Actor Executor handles WebAssembly component instantiation and execution:

- **Relationship with WASM Components**: Loads and instantiates WebAssembly components
- **Relationship with Host Functions**: Provides host functions to running components
- **Relationship with Runtime**: Reports execution results back to the Runtime

### Store System

The Store provides content-addressable storage for actor state:

- **Relationship with Actors**: Provides state storage and retrieval for actors
- **Relationship with Runtime**: Coordinates with the Runtime for state management
- **Relationship with Event Chain**: State changes are recorded in the Event Chain

### Event Chain

The Event Chain records all system events:

- **Relationship with Runtime**: Receives events from the Runtime
- **Relationship with Store**: Records state changes from the Store
- **Relationship with CLI Tools**: Provides data for inspection and debugging

## Secondary Components

### CLI

The CLI provides user interaction with the Theater system:

- **Relationship with Runtime**: Sends commands to the Runtime
- **Relationship with Event Chain**: Retrieves and displays events
- **Relationship with Store**: Accesses stored content

### Handlers

Handlers extend actor functionality:

- **Relationship with Actor Executor**: Registered with the Executor
- **Relationship with Runtime**: Managed by the Runtime
- **Relationship with Event Chain**: Handler invocations are recorded

## Component Interaction Patterns

### Creation Flow

The sequence of component interactions during actor creation:

1. CLI or parent actor requests actor creation
2. Runtime processes request
3. Store retrieves component bytes
4. Actor Executor instantiates component
5. Runtime initializes actor state
6. Event Chain records creation

### Message Processing Flow

The sequence of component interactions during message processing:

1. Message arrives at Runtime
2. Runtime records message in Event Chain
3. Runtime delivers message to target actor
4. Actor Executor invokes appropriate handler
5. Actor may access state via Store
6. Actor response is recorded in Event Chain
7. Response is delivered to sender

### Failure Handling Flow

The sequence of component interactions during failure handling:

1. Actor Executor detects failure
2. Runtime records failure in Event Chain
3. Runtime notifies supervisor
4. Supervisor decides on action
5. Runtime implements supervisory action
6. Event Chain records recovery action

Understanding these component relationships and interaction patterns provides insight into how Theater operates internally and how its various parts work together to create a cohesive system.
