# Core Concepts

This section introduces the fundamental concepts behind Theater. Rather than diving into implementation details, the focus here is on the key ideas and principles that make Theater unique.

Theater is built on three core pillars:

## 1. WebAssembly Components
By leveraging WebAssembly Components as its foundation, Theater provides strong sandboxing, deterministic execution, and language-agnostic interfaces. This ensures that code runs consistently regardless of the environment and cannot access resources it shouldn't.

- [Sandboxing and Security](wasm-components/sandboxing.md)
- [Deterministic Execution](wasm-components/determinism.md)
- [Component Interfaces](wasm-components/interfaces.md)

## 2. Actor Model with Supervision
The actor model provides a natural way to structure concurrent systems. Each actor is a self-contained unit with isolated state that communicates through message passing. Theater adds Erlang-inspired supervision for robust failure handling.

- [Actor Basics](actor-model/actors.md)
- [Actor Communication](actor-model/communication.md)
- [Supervision System](actor-model/supervision.md)

## 3. Complete Traceability
Every interaction with an actor is recorded in a cryptographically-linked chain of events. This gives Theater powerful capabilities for debugging, verification, and recovery.

- [Event Chain](traceability/event-chain.md)
- [State Verification](traceability/verification.md)
- [Debugging and Inspection](traceability/debugging.md)

Understanding these core concepts will help you build reliable, maintainable systems with Theater, even as the ecosystem of AI-generated code continues to grow.
