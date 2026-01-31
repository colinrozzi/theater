# Core Concepts

Theater is built on three fundamental pillars that work together to create a system that is secure, reliable, and transparent. This section explains what Theater is and the key concepts that make it the ideal infrastructure for AI agent systems.

Before diving into the pillars, read [The Deterministic Boundary](./the-boundary.md) — it articulates the foundational insight that connects all three: WASM as a deterministic box in a non-deterministic world, the chain as the record of what crosses the boundary, and why this framing makes the rest of the architecture make sense.

## The Three Pillars of Theater

### [WebAssembly Components & Sandboxing](./wasm-components.md)

WebAssembly provides the foundation for Theater's security and capability controls:

- Strong security boundaries through sandboxing
- Deterministic execution for reproducible behavior
- Language-agnostic agent implementation
- Capability-based security model for precise access control

### [Actor Model & Supervision](./actor-model.md)

The Actor Model enables Theater's approach to agent organization, communication, and fault tolerance:

- Agents as independent, isolated entities
- Message-passing for all agent-to-agent communication
- Private state management for each agent
- Hierarchical supervision for reliable agent systems

### [Traceability & Verification](./traceability.md)

Traceability ensures that all agent actions are observable, auditable, and debuggable:

- Event Chain capturing every agent action
- Deterministic replay for verification and debugging
- State management for consistent agent snapshots
- Comprehensive tools for inspection and analysis

## How The Pillars Work Together

These three pillars complement each other to create Theater's unique properties for agent systems:

- **WebAssembly + Actor Model**: Provides secure agents with clear communication patterns
- **WebAssembly + Traceability**: Enables deterministic replay and verification of agent behavior
- **Actor Model + Traceability**: Supports failure diagnosis and recovery in complex agent systems

By understanding these core concepts, you'll have a solid foundation for building reliable, secure, and transparent AI agent systems with Theater.
