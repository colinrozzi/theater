# Theater

## Overview
Runtime V2 is a redesigned actor system that enables state management, verification, and flexible interaction patterns for WebAssembly components. The system is built around a core concept: actors that maintain verifiable state and can interact with the outside world in various ways.

## Key Concepts

### Actors
An actor in this system is a WebAssembly component that:
- Maintains state
- Responds to inputs by producing outputs and updating state
- Participates in a verifiable hash chain of all state changes
- Can interact with the outside world through various interfaces

### Hash Chain
All actor state changes are recorded in a verifiable hash chain. This enables:
- Complete history of how an actor reached its current state
- Verification of state transitions
- Ability to replay and audit state changes
- Cross-actor state verification

### Flexible Interfaces
The system is designed to support multiple ways for actors to interact with the outside world:
- Message passing between actors
- HTTP server capabilities
- Future interfaces (filesystem, timers, etc.)

## Core Architecture

### ActorInput and ActorOutput
These enums represent all possible ways data can flow into and out of an actor:
```rust
pub enum ActorInput {
    Message(Value),
    HttpRequest { ... },
    // Future input types
}

pub enum ActorOutput {
    Message(Value),
    HttpResponse { ... },
    // Future output types
}
```

This design:
- Makes all possible interactions explicit
- Enables type-safe handling of different interaction patterns
- Allows easy addition of new interaction types
- Ensures consistent chain recording of all inputs

### Actor Trait
The core interface that all actors must implement:
```rust
pub trait Actor {
    fn init(&self) -> Result<Value>;
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;
    fn verify_state(&self, state: &Value) -> bool;
}
```

Key design decisions:
- Use of serde_json::Value for state enables flexible state representation
- Single handle_input method unifies all interaction types
- Explicit state verification support
- Clear initialization pattern

### ActorRuntime
Manages the core actor lifecycle:
- State management
- Chain recording
- Input handling
- State verification

### Interfaces
The ActorInterface trait enables multiple ways to expose actors:
```rust
pub trait ActorInterface {
    type Config;
    fn new(config: Self::Config) -> Result<Self> where Self: Sized;
    fn start(self, runtime: ActorRuntime<impl Actor>) -> Result<()>;
}
```

This allows:
- Clean separation between core actor logic and exposure mechanisms
- Multiple simultaneous interfaces per actor
- Easy addition of new interface types

## Roadmap

### Phase 1: Core Implementation
1. Complete basic message-passing interface
   - Implement MessageInterface
   - Port existing actor-to-actor communication
   - Add tests for basic messaging

2. Add WASM component integration
   - Create WasmActor implementation
   - Add manifest parsing
   - Implement host functions
   - Test with simple components

### Phase 2: HTTP Support
1. Implement HttpInterface
   - HTTP server setup
   - Request/response handling
   - Chain recording for HTTP interactions

2. Create HTTP actor examples
   - Simple static file server
   - REST API example
   - WebSocket support investigation

### Phase 3: Enhanced Features
1. Add more interface types
   - Filesystem access
   - Timer/scheduling
   - Database connections

2. Improve chain verification
   - Cross-actor verification
   - Chain pruning strategies
   - Performance optimizations

3. Development tools
   - Chain visualization
   - Actor debugging tools
   - State inspection utilities

## Contributing
When adding new features:
1. Consider how they fit into the core abstractions
2. Ensure all state changes are properly recorded
3. Add appropriate tests
4. Update documentation

## Design Principles
1. **Explicit over implicit**: All possible interactions should be explicitly modeled in the type system.
2. **Verifiable state**: Every state change must be recorded and verifiable.
3. **Extensible interfaces**: New ways of interacting with actors should be easy to add.
4. **Clean separation**: Core actor logic should be separate from interface mechanisms.
5. **Type safety**: Use the type system to prevent invalid interactions.

## Development Setup
[To be added: Development environment setup, build instructions, test running]
