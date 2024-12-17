# Theater: Core Concepts

## Foundation

Theater is built on two fundamental technologies that enable trustworthy distributed systems:

1. **WebAssembly Components** - Providing:
   - Deterministic execution
   - Language-agnostic implementation
   - Strong isolation boundaries
   - Near-native performance
   - Cross-platform portability

2. **Hash Chains** - Enabling:
   - Verifiable state history
   - Self-contained trust
   - Complete auditability
   - State reproducibility

## The Big Idea

Traditional distributed systems often try to enforce trust through centralized authorities or complex consensus mechanisms. Theater takes a different approach: trust is built from the ground up, with each actor maintaining its own verifiable history.

### Key Principles

1. **Self-Verifying Actors**
   - Each actor maintains its own hash chain
   - Every state change is recorded and linked
   - State can be verified by replaying the chain
   - No central authority needed for verification

2. **Deterministic Execution**
   - WebAssembly ensures consistent execution
   - Same inputs always produce same outputs
   - State changes are reproducible
   - Behavior is predictable across platforms

3. **Composable Trust**
   - Actors can verify each other's state
   - Trust relationships emerge naturally
   - System-wide properties can be proven
   - No global consensus required

## Core Architecture

```
┌─────────────────────────┐
│     WebAssembly Actor   │
├─────────────────────────┤
│    State Management     │◄─────┐
└───────────┬─────────────┘      │
            │                    │
            ▼                    │
┌─────────────────────────┐      │
│      Event Chain        │      │
│  ┌─────┐ ┌─────┐ ┌─────┐│     │
│  │Event│►│Event│►│Event││     │
│  └─────┘ └─────┘ └─────┘│     │
└───────────┬─────────────┘     │
            │                    │
            ▼                    │
┌─────────────────────────┐      │
│    State Transition     ├─────┘
└─────────────────────────┘
```

### How It Works

1. Each actor is a WebAssembly component with:
   - Defined state structure
   - Clear input/output interfaces
   - Deterministic behavior

2. Every state change is:
   - Triggered by an input
   - Recorded as an event
   - Linked to previous events
   - Produces verifiable output

3. Hash chains provide:
   - Complete event history
   - Verifiable state transitions
   - Audit capabilities
   - Replay functionality

## Practical Implementation

The actual implementation of Theater focuses on:

1. **Minimal Core**
   - WebAssembly runtime integration
   - Hash chain management
   - Basic actor lifecycle
   - State verification

2. **Clean Interfaces**
   - Simple event structure
   - Clear state transitions
   - Verifiable inputs/outputs
   - Composable components

3. **Extension Points**
   - Communication protocols (HTTP, WebSocket, etc.)
   - Storage backends
   - Verification strategies
   - Custom actor types

## Design Goals

1. **Simplicity**
   - Focus on core mechanisms
   - Clear, understandable architecture
   - Minimal external dependencies
   - Easy to reason about

2. **Verifiability**
   - All state changes are recorded
   - History is immutable
   - Chains are verifiable
   - Trust is built-in

3. **Flexibility**
   - Multiple languages via WebAssembly
   - Various communication patterns
   - Different storage options
   - Extensible architecture

## Summary

Theater provides a new foundation for building distributed systems by combining WebAssembly's deterministic execution with the verifiable history of hash chains. This creates a system where trust emerges naturally from the properties of individual actors rather than being imposed from above.