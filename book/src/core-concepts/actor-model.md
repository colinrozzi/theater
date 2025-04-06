# Actor Model & Supervision

The Actor Model provides Theater's approach to concurrency, isolation, and fault tolerance. This pillar explains how Theater uses actors and supervision hierarchies to create resilient systems.


Ultimately, the actor model provides isolation. Each actor runs in its own process, with its own memory space. All of the benefits of the actor system are derived from this isolation. Actors can be restarted, stopped, or even replaced with new versions without affecting the rest of the system. This allows for a high degree of flexibility and adaptability in the face of change. Errors can be contained to the actor that caused them, and actors can be restarted or replaced without affecting the rest of the system. Actors can be developed and deployed independently, as the system is running, so the system can be continuously improved and updated without downtime. Also, by managing the state of the actors in the runtime, actors can be taken down and re-deployed without losing their state, meaning the state of an application can be preserved while the application is in active development, as long as breaking changes are not made. Finally, the actor model allows for a high degree of parallelism, as actors can be run in parallel on different threads or even different machines. This allows for a high degree of scalability and performance, as the system can take advantage of all available resources to process messages and perform work.


## What is the Actor Model?

The Actor Model is a conceptual framework for designing concurrent systems:

- **Actors as Fundamental Units**: Every computation is encapsulated in an actor
- **Message-Passing Communication**: Actors communicate only through messages
- **Isolated State**: Each actor manages its own private state
- **Concurrent Execution**: Actors can process messages concurrently

In Theater, each WebAssembly component instance is an actor with a well-defined lifecycle and communication pattern.

## Actor Lifecycle

Actors in Theater have a clear lifecycle:

1. **Creation**: Actors are created by other actors or the system
2. **Initialization**: Actor initializes its state and registers message handlers
3. **Operation**: Actor processes messages and performs work
4. **Termination**: Actor is stopped, either gracefully or due to failure

The system tracks this lifecycle and ensures proper resource management at each stage.

## Message-Passing Communication

Communication in Theater happens exclusively through messages:

- **Asynchronous**: Senders don't wait for receivers to process messages
- **Immutable**: Messages cannot be modified after sending
- **Explicit**: All communication is visible and traceable
- **Typed**: Messages follow defined interface specifications

This message-passing approach eliminates many concurrency issues that plague shared-state systems.

## Hierarchical Supervision

Inspired by Erlang/OTP, Theater implements a supervision system:

- **Supervision Trees**: Actors are arranged in parent-child hierarchies
- **Fault Isolation**: Failures in one actor don't affect siblings
- **Recovery Strategies**: Parents decide how to handle child failures
- **Escalation**: Unhandled failures can be escalated up the tree

### Supervision Strategies

Parents can implement different strategies for handling child failures:

- **Restart**: Restart the failed child, preserving its identity
- **Stop**: Terminate the failed child
- **Escalate**: Report the failure to the parent's supervisor
- **Custom**: Implement application-specific recovery logic

## Benefits for Theater

The Actor Model with supervision provides Theater with:

1. **Concurrency**: Natural way to express parallel computations
2. **Fault Tolerance**: Localize and recover from failures
3. **Scalability**: Distribute actors across resources
4. **Simplicity**: Clear communication patterns reduce complexity
5. **Resilience**: Systems can continue despite partial failures

These properties make Theater particularly well-suited for building resilient systems that can recover from failures automatically, which is essential when running potentially unreliable AI-generated code.
