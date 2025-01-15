# Why Theater?

Theater is more than just another actor system - it's an exploration into making distributed systems more debuggable, reproducible, and verifiable. The project introduces several key innovations:

## Core Concepts

### State Tracking Through Hash Chains

At the heart of Theater is a novel approach to state management in WebAssembly components. Every interaction between host and WebAssembly is tracked in a hash chain, creating a complete and verifiable history of state transitions. This is similar to a blockchain or hashgraph event, but at the granular level of individual actor interactions.

Key benefits:
- **Complete State History**: Every state change is recorded and linked to its parent state
- **Deterministic Replay**: Because WebAssembly is sandboxed and deterministic, any sequence of events can be replayed exactly on any system
- **Enhanced Debugging**: Improved tracing across WebAssembly component boundaries, addressing a key pain point in current WebAssembly development
- **Verifiable State**: Each state transition is cryptographically linked to its predecessors, ensuring integrity of the state history

### Unified Message Interface

Theater simplifies actor interactions through a unified JSON-based message interface:
- All actors implement a single, consistent interface
- Messages and state are passed as JSON
- Each handler returns both the new state and response message
- This uniformity enables flexible composition while maintaining simplicity

### Supervision & Lifecycle Management

Theater implements a robust supervision system where:
- Actors can spawn and manage other actors
- Parent actors receive lifecycle notifications about their children
- This creates clear hierarchies of responsibility and error handling

## Why This Matters

Traditional distributed systems often struggle with:
1. **State Reproducibility**: It's hard to reproduce exact conditions that led to a bug
2. **Cross-Component Debugging**: Tracing issues across component boundaries is challenging
3. **State Verification**: Ensuring state hasn't been tampered with is complex

Theater addresses these challenges through its hash chain approach, providing:
- Guaranteed reproduction of any state or error condition
- Complete visibility into state transitions
- Cryptographic verification of state history
- Clear patterns for actor supervision and management

## Future Implications

The hash chain approach to state management opens up interesting possibilities:
- **Distributed Verification**: Nodes can verify each other's state transitions
- **Time Travel Debugging**: Ability to move backwards through state history
- **State Merkle Proofs**: Prove properties about actor state histories
- **Automated Testing**: Replay real production scenarios in tests

## Target Use Cases

Theater is particularly well-suited for:
- Systems requiring audit trails of all state changes
- Applications needing guaranteed reproducibility of bugs
- Distributed systems where state verification is critical
- Complex actor systems requiring robust supervision patterns

## Technical Foundation

Built on WebAssembly component model, Theater leverages:
- WebAssembly's sandboxing for deterministic execution
- Component model for clean interface boundaries
- Modern Rust for performance and safety
- Nix for reproducible development environments