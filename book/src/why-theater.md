# Why Theater?

Theater is an exploration into making systems that are more trackable, reproducable, and predictable. At a high level, it does this by drawing a box around every actor, only allowing certain interacions with the outside world, and completely tracking every interaction.

Theater is attempting to make a system for the new world of llm driven applications. With new tools come new requirements. The Theater is a collection of ideas that come together to provide an environment for building these new applications.

With llm written applications, we are fundamentally executing untrusted code on our computers. This means we have to build into our systems ways to ensure our programs behave the way that we expect them to, and provide ways to verify that they are doing so and methods to debug them when they are not.

To ensure the safety of our systems, Theater leverages the WebAssembly runtime to provide a sandboxed, deterministic, and portable environment for our actors.

The Theater uses the Actor Model to provide a way to structure our applications. Theater's actor model is highly inspired by the erlang actor model, and hopes to inherit its robustness, scalability, and fault tolerance.

To do this, Theater leverages many existing ideas and technologies.

## Actor Model
The Actor Model is a model of computation 

## WebAssembly
Each actor in the system is a WebAssembly Component. A huge amount of the heavy lifting in the system is done by the WebAssembly runtime. By using WebAssembly, each actor is sandboxed, deterministic, and portable. An actor is just a WebAssembly Component that implements a specific set of interfaces to interact with the host system. The WebAssembly component model has not yet reached a stable state, and many things like async, something like a package manager, and many language bindings are still in development. As the Component model approaches stability, Theater will evolve alongside it and will be making changes, especially to the host interfaces.



## Core Concepts

### State Tracking Through Hash Chains

At the heart of Theater is a novel approach to state management in WebAssembly components. Every interaction between host and WebAssembly is tracked in a hash chain, creating a complete and verifiable history of state transitions. 

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
