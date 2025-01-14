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
┌─────────────────────────┐         ┌─────────────────────┐
│      Handler Layer      │         │   WASM Component    │
│  ┌─────────┬─────────┐ │◄────────│                     │
│  │  HTTP   │ Message │ │         └─────────┬───────────┘
│  │ Server  │ Server  │ │                   │
└──┴─────────┴─────────┴─┘                   │
           │                                  │
           │              ┌──────────────────▼┐
     ┌─────▼──────────┐  │   Actor Process   │
     │ Actor Runtime  │◄─┤                    │
     └───────┬────────┘  └──────────┬────────┘
             │                       │
     ┌───────▼────────┐    ┌────────▼───────┐
     │  Chain Handler │    │   Actor State   │
     └───────┬────────┘    └────────────────┘
             │
     ┌───────▼────────┐
     │  Hash Chain    │
     └────────────────┘
```

### Key Components

1. **Handler Layer**
   - `HttpServerHost`: Handles incoming HTTP requests
   - `MessageServerHost`: Manages actor-to-actor messaging
   - Routing incoming requests to appropriate actors

2. **Actor Runtime**
   - `ActorRuntime`: Manages component lifecycle
   - `RuntimeComponents`: Core runtime infrastructure
   - Chain request handling
   - Component initialization

3. **Actor Process**
   - State management
   - Event handling
   - Message processing
   - Chain synchronization

4. **Chain Handler**
   - Chain entry management
   - Event recording
   - State verification
   - Chain integrity

5. **WebAssembly Integration**
   - Component loading
   - Capability management
   - Host function provisioning
   - Interface verification

## Features

### Actor State Management
- Complete state history through HashChain
- Verifiable state transitions
- JSON-based state storage
- Atomic updates

### Event System
- Structured event types
  - HttpRequest
  - ActorMessage
  - StateChange
- Complete event history
- Event verification
- Chain-based ordering

### Multiple Interface Types
- HTTP server capabilities
- Actor-to-actor messaging
- Extensible handler system
- Common message format

### WebAssembly Integration
- Component model support
- Capability-based security
- Interface contracts
- Runtime isolation

## Implementation Notes

### Actor Interface
```rust
pub trait Actor {
    async fn init(&self) -> Result<Value>;
    async fn handle_event(&self, state: Value, event: Event) 
        -> Result<(Value, Event)>;
    async fn verify_state(&self, state: &Value) -> bool;
}
```

### Event Structure
```rust
pub struct Event {
    pub type_: String,
    pub data: Value,
}

pub struct ChainEntry {
    pub event: Event,
    pub parent: Option<String>,
}
```

### Handler Types
```rust
pub enum Handler {
    MessageServer(MessageServerHost),
    HttpServer(HttpServerHost),
}

pub enum ChainRequestType {
    GetHead,
    GetChainEntry(String),
    GetChain,
    AddEvent { event: Event },
}
```

## Quick Start

1. Install dependencies
```bash
cargo build
```

2. Create an actor manifest (actor.toml):
```toml
name = "my-actor"
component_path = "path/to/actor.wasm"

[interface]
implements = "ntwk:simple-actor/actor"

[[handlers]]
type = "Http-server"
config = { port = 8080 }

[[handlers]]
type = "Message-server"
config = { port = 8081 }
```

3. Run the actor:
```bash
cargo run -- --manifest path/to/manifest.toml
```

## Development Status

Current features:
1. Basic actor system with state management
2. HTTP and message server handlers
3. Chain-based event recording
4. WebAssembly component support
5. JSON-based state and messaging

In progress:
1. Enhanced chain verification
2. Additional handler types
3. More complex component interactions

## Contributing

When adding features:
1. Consider the actor model
2. Ensure state verification
3. Maintain chain integrity
4. Add appropriate tests
5. Update documentation

## Next Steps

### Phase 1: Core Stability
- Enhance error handling
- Improve state verification
- Add chain optimization

### Phase 2: Extended Features
- Additional handler types
- Enhanced HTTP capabilities
- WebSocket support

### Phase 3: Developer Tools
- Chain visualization
- State inspection tools
- Development utilities