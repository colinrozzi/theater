# Actor Model & Supervision

The Actor Model is the second pillar of Theater, providing a robust framework for concurrency, state management, communication, and fault tolerance. Inspired by systems like Erlang/OTP, Theater implements actors with hierarchical supervision to build resilient and scalable applications.

## Actors: Isolated Units of Computation

In Theater, the fundamental unit of computation and state is the *actor*. Each actor is an independent entity characterized by:

1.  **Private State**: An actor maintains its own internal state, which cannot be directly accessed or modified by other actors. This isolation is a cornerstone of the model.
2.  **Mailbox**: Each actor has a mailbox where incoming messages are queued.
3.  **Behavior**: An actor defines how it processes incoming messages, potentially changing its state, sending messages to other actors, or creating new actors.
4.  **Address**: Actors have unique addresses used to send messages to them.

Crucially, in Theater, each actor instance corresponds to a running [WebAssembly Component](./wasm-components.md), benefiting from the security and isolation guarantees provided by Wasm.

## Communication via Asynchronous Message Passing

All interaction between actors in Theater occurs exclusively through asynchronous message passing.

-   **No Shared Memory**: Actors do not share memory. To communicate, an actor sends an immutable message to another actor's address.
-   **Asynchronous & Non-Blocking**: When an actor sends a message, it does not wait for the recipient to process it. This allows actors to work concurrently without blocking each other.
-   **Location Transparency**: Actors communicate using addresses, without needing to know the physical location (e.g., thread, process, machine) of the recipient actor. (Note: While Theater currently runs actors within a single process, the model allows for future distribution).
-   **Explicit and Traceable**: All interactions are explicit message sends, which are captured by Theater's [Traceability](./traceability.md) system.

This communication style simplifies concurrency management, eliminating many common issues like race conditions and deadlocks found in shared-state concurrency models.

## Isolation: The Core Benefit

The strict isolation provided by the Actor Model (actors having their own state and communicating only via messages) is the source of many of Theater's resilience and flexibility features:

-   **Fault Isolation**: If an actor encounters an error and crashes, the failure is contained within that actor. It does not directly affect the state or operation of other actors in the system.
-   **Independent Lifecycle**: Actors can be started, stopped, restarted, or even upgraded independently without necessarily affecting unrelated parts of the system.
-   **State Management**: Because state is encapsulated, Theater can manage actor state persistence and recovery more easily. State can potentially be preserved across restarts or deployments under certain conditions.

## Hierarchical Supervision for Fault Tolerance

Theater adopts Erlang/OTP's concept of hierarchical supervision to manage failures gracefully.

-   **Supervision Trees**: Actors are organized into a tree structure where parent actors *supervise* their child actors.
-   **Monitoring**: Supervisors monitor the health of their children.
-   **Recovery Strategies**: When a child actor fails (e.g., crashes due to an unhandled error), its supervisor is notified and decides how to handle the failure based on a defined strategy. Common strategies include:
    * **Restart**: Restart the failed child, potentially restoring its last known good state.
    * **Stop**: Terminate the failed child permanently if it's deemed unrecoverable or non-essential.
    * **Escalate**: If the supervisor cannot handle the failure, it can fail itself, escalating the problem to *its* supervisor.
    * **Restart Siblings**: In some cases, a failure in one child might require restarting other related children (siblings).

This structure allows developers to define how the system should react to failures, building self-healing capabilities directly into the application architecture. Error handling becomes a primary architectural concern, rather than an afterthought.

## Benefits for Theater

Integrating the Actor Model with supervision provides Theater with:

1.  **Simplified Concurrency**: A natural model for handling many simultaneous operations without shared-state complexity.
2.  **Enhanced Fault Tolerance**: The ability to contain failures and automatically recover parts of the system.
3.  **Scalability**: Actors can potentially be distributed across cores or machines to handle increased load (though current implementation specifics may vary).
4.  **Resilience**: Systems can remain partially or fully operational even when individual actors fail.
5.  **Maintainability**: Actors can often be developed, deployed, and updated independently, facilitating continuous improvement without system downtime (depending on the nature of changes).

Combined with Wasm components, the Actor Model allows Theater to manage potentially unreliable code units within a structure designed for resilience and recovery.
