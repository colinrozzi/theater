# Core Concepts

Theater is built on three fundamental pillars that work together to create a system that is secure, reliable, and transparent. This section explains what Theater is and the key concepts that make it unique.

## The Three Pillars of Theater

### [WebAssembly Components & Sandboxing](./wasm-components.md)

WebAssembly provides the foundation for Theater's security and determinism guarantees:

- Security boundaries through sandboxing
- Deterministic execution
- Language-agnostic components
- Capability-based security model

### [Actor Model & Supervision](./actor-model.md)

The Actor Model enables Theater's approach to concurrency, isolation, and fault tolerance:

- Actors as fundamental units of computation
- Message-passing for all communication
- Isolated state management
- Hierarchical supervision for fault tolerance

### [Traceability & Verification](./traceability.md)

Traceability ensures that Theater systems are transparent, auditable, and debuggable:

- Event Chain capturing all system activities
- Deterministic replay for verification
- State management for consistent snapshots
- Comprehensive tools for inspection and debugging

## How The Pillars Work Together

These three pillars complement each other to create Theater's unique properties:

- **WebAssembly + Actor Model**: Provides strong isolation with clear communication patterns
- **WebAssembly + Traceability**: Enables deterministic replay and verification
- **Actor Model + Traceability**: Supports fault diagnosis and recovery

By understanding these core concepts, you'll have a solid foundation for using Theater effectively and making the most of its capabilities.
