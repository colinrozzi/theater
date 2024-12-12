# Theater

## Overview
Theater is a redesigned actor system that enables state management, verification, and flexible interaction patterns for WebAssembly components. The system is built around a core concept: actors that maintain verifiable state and can interact with the outside world in various ways.

## Why Theater Exists

Theater is designed to solve several key challenges in distributed systems:

1. **State Verification**: In distributed systems, verifying the accuracy and authenticity of state changes is crucial. Theater provides a complete, auditable chain of all state transitions.

2. **Reproducibility**: By tracking all inputs and state changes in a hash chain, any actor's current state can be independently verified and reproduced by replaying its history.

3. **Flexible Interaction**: Modern systems need to interact in multiple ways - through direct messages, HTTP APIs, and other protocols. Theater provides a unified framework for handling these diverse interaction patterns while maintaining state verification.

4. **Component Isolation**: Using WebAssembly components provides strong isolation and security guarantees while enabling actors to be written in any language that compiles to Wasm.

## System Architecture

```
┌─────────────────┐         ┌─────────────────┐
│   HTTP Server   │         │  WASM Component │
│                 │◄────────│                 │
└────────┬────────┘         └────────┬────────┘
         │                           │
         │         ┌─────────────────┤
         │         │                 │
    ┌────▼─────────▼────┐     ┌─────┴──────┐
    │    Runtime Core   │◄────┤  Manifest   │
    │                  │     │  Parser     │
    └────────┬─────────┘     └────────────┘
             │
    ┌────────▼─────────┐
    │    Hash Chain    │
    │                 │
    └────────┬─────────┘
             │
    ┌────────▼─────────┐
    │  State Storage   │
    │    (Memory)      │
    └──────────────────┘
```

### Key Components

1. **Runtime Core**: Manages actor lifecycle, state, and chain recording
   - State management
   - Chain recording
   - Component lifecycle
   - Message routing

2. **Hash Chain**: Records and verifies all state changes
   - Immutable history
   - State verification
   - Chain integrity
   - Audit capability

3. **WebAssembly Integration**: Manages component execution
   - Component loading
   - State isolation
   - Message handling
   - Contract verification

4. **Network Interface**: Exposes actors to the world
   - HTTP endpoints
   - Message routing
   - Chain access
   - State queries

## Features

### Actor State Management
- Complete state history
- Verifiable transitions
- Contract enforcement
- State isolation

### Hash Chain Verification
- Immutable record
- State verification
- Chain integrity
- Audit support

### Multiple Interface Types
- Actor-to-actor messaging
- HTTP server capabilities
- HTTP client capabilities
- Extensible interface system

## Design Principles

1. **Explicit over Implicit**
   - All possible interactions explicitly modeled
   - Clear state transitions
   - Defined contracts
   - Transparent routing

2. **Verifiable State**
   - Every change recorded
   - Chain verification
   - Contract enforcement
   - State validation

3. **Extensible Interfaces**
   - Multiple interaction patterns
   - Easy to add new interfaces
   - Protocol abstraction
   - Clean separation

4. **Clean Separation**
   - Modular design
   - Interface independence
   - Clear boundaries
   - Minimal coupling

5. **Type Safety**
   - Strong typing
   - Contract verification
   - Message validation
   - State checking

## Implementation Notes

### State Management
```rust
pub trait Actor {
    fn init(&self) -> Result<Value>;
    fn handle_input(&self, input: ActorInput, state: &Value) -> Result<(ActorOutput, Value)>;
    fn verify_state(&self, state: &Value) -> bool;
}
```

### Message Processing
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

## Quick Start

1. Install Rust and cargo
```bash
git clone https://github.com/colinrozzi/theater.git
cd theater
cargo build
```

2. Create an actor manifest (actor.toml):
```toml
name = "my-actor"
component_path = "path/to/actor.wasm"

[interface]
implements = "ntwk:simple-actor/actor"
requires = []

[[handlers]]
type = "Http"
config = { port = 8080 }
```

3. Run an actor:
```bash
cargo run -- --manifest path/to/your/manifest.toml
```

## Development Status

Current work focuses on:
1. Manifest parsing and runtime initialization
2. Actor-to-actor communication
3. HTTP interface implementation

See [Building Actors](docs/building-actors.md) for detailed development documentation.

## Contributing

When adding new features:
1. Consider how they fit into the core abstractions
2. Ensure all state changes are properly recorded
3. Add appropriate tests
4. Update documentation

## Next Steps

### Phase 1: Core Implementation
- Complete basic message-passing interface
- Add WASM component integration
- Implement manifest parsing

### Phase 2: HTTP Support
- Implement HTTP server interface
- Create example HTTP actors
- Add WebSocket support

### Phase 3: Enhanced Features
- Add more interface types (filesystem, timers, etc.)
- Improve chain verification
- Develop debugging and visualization tools
