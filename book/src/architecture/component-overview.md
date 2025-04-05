# Theater Architecture

This document describes the technical architecture of Theater, explaining how its components are structured and interact to create a secure, reliable runtime for WebAssembly actors.

## System Overview

Theater follows a layered architecture with clear separation of responsibilities between components:

![Theater Architecture Diagram](/path/to/diagram.png)

## Core Components

### Theater Runtime

The **Theater Runtime** (`theater_runtime.rs`) is the central coordination component that manages the overall actor system. It's responsible for:

- Managing the global actor pool and lifecycle events
- Routing messages between actors or between external clients and actors
- Orchestrating structured communication through "Channels"
- Propagating failure notifications through the supervision hierarchy
- Coordinating state persistence and restoration

The Theater Runtime exists as a singleton within a Theater instance and maintains references to all active actors.

### Theater Server

The **Theater Server** (`theater_server.rs`) exposes Theater's functionality to the outside world through network interfaces. It:

- Listens for management commands over HTTP/WebSocket
- Authenticates and authorizes external requests
- Processes commands to start, stop, inspect, or message actors
- Communicates with the Theater Runtime to execute these requests
- Streams events and logs back to clients

The Server is configurable to run with different transport protocols and security settings.

### Actor Runtime

Each actor gets its own **Actor Runtime** (`actor_runtime.rs`) instance, which:

- Loads and initializes the actor's WebAssembly component
- Provisions host capabilities based on the manifest configuration
- Manages the actor's entire lifecycle (init, message handling, shutdown)
- Maintains the actor's state and event chain
- Enforces resource quotas and limits

The Actor Runtime creates a secure boundary around each actor, controlling what it can access and how it interacts with the rest of the system.

### Actor Executor

The **Actor Executor** (`actor_executor.rs`) handles the low-level execution of WebAssembly code:

- Creates and configures the Wasmtime execution environment
- Manages the Wasmtime Store containing the actor's memory and state
- Links the permitted host functions to the WebAssembly module
- Executes specific actor functions when requested
- Translates between host and WebAssembly data representations
- Enforces execution limits like fuel (instruction counting)

Multiple actor instances of the same type can share a single compiled module but will have separate Stores for isolation.

### WebAssembly Interface

The **WebAssembly Interface** (`wasm.rs`) provides the critical bridge between the host runtime and WebAssembly components. It:

- Handles loading and instantiation of WebAssembly components
- Manages type-safe function calls across the WebAssembly boundary
- Implements memory isolation and safety checks
- Tracks resource usage of WebAssembly instances
- Provides error handling for WebAssembly operations

The interface consists of several key abstractions:

1. **ActorComponent**: Represents a loaded WebAssembly component ready for instantiation
2. **ActorInstance**: An instantiated component with registered functions
3. **TypedFunction**: A trait for type-safe function calls with various signatures
4. **MemoryStats**: Structure for tracking memory usage of WebAssembly components

This interface ensures that all interactions with WebAssembly code are safe, secure, and properly tracked.

### Actor Manifest

The **Actor Manifest** (`ManifestConfig`) defines what an actor is and how it should run:

- Specifies the path to the actor's WebAssembly component file
- Declares required capabilities (HTTP, filesystem, etc.) and their configurations
- Sets up handlers for different types of inputs (messages, HTTP requests)
- Defines supervision policies and recovery strategies
- Specifies state persistence requirements

Manifests are typically stored as TOML files and loaded by the Theater Runtime during actor creation.

### Chain System

The **Chain** system (`chain/mod.rs`) implements Theater's traceability features:

- Records all inputs and outputs crossing the WebAssembly boundary
- Creates a cryptographically verified chain of events and state changes
- Provides the foundation for deterministic replay and verification
- Supports both in-memory and persistent storage of event history
- Implements rollback and restoration of actor state

The Chain system is integrated with the Actor Runtime to automatically track all interactions.

## Data Flow

### Actor Lifecycle

1. An actor begins with a manifest being submitted to the Theater Server
2. The Server forwards the manifest to the Theater Runtime
3. The Runtime creates a new Actor Runtime instance
4. The Actor Runtime uses the Actor Executor to load the WebAssembly component
5. The Executor initializes the actor by calling its `init` function
6. The Actor Runtime records the initialization in the Chain
7. The actor is now ready to process messages and requests

### Message Handling

1. A message arrives at the Theater Server from an external client
2. The Server forwards it to the Theater Runtime with the target actor ID
3. The Runtime locates the appropriate Actor Runtime
4. The Actor Runtime:
   - Records the incoming message in the Chain
   - Uses the Actor Executor to call the actor's message handler function
   - Records any capability invocations and their results
   - Records the handler's response in the Chain
5. The response is returned to the client through the Theater Runtime and Server

### Capability Invocation

1. The actor's WebAssembly code calls a host function (e.g., HTTP request)
2. The Actor Executor intercepts this call and:
   - Verifies the capability is permitted according to the manifest
   - Translates WebAssembly parameters to host types
   - Records the invocation request in the Chain
3. The Actor Runtime processes the capability request
4. The result is:
   - Recorded in the Chain
   - Translated back to WebAssembly types
   - Returned to the actor's code

### Supervision and Recovery

1. If an actor fails during execution:
   - The Actor Executor catches the exception
   - The Actor Runtime records the failure in the Chain
   - The Theater Runtime is notified of the failure
2. The Theater Runtime:
   - Identifies the actor's supervisor (parent actor or the system)
   - Notifies the supervisor of the failure
3. Based on the supervision strategy, the system may:
   - Restart the actor with its previous state
   - Terminate the actor
   - Escalate the failure to a higher-level supervisor

## Implementation Details

### WebAssembly Component Model

Theater uses the WebAssembly Component Model to define actor interfaces:

```rust
// Inside wasm.rs
fn load_component(path: &Path, engine: &Engine) -> Result<Component, Error> {
    let component_bytes = std::fs::read(path)?;
    let component = Component::new(engine, component_bytes)?;
    Ok(component)
}
```

Components implement standardized interfaces defined using WIT (WebAssembly Interface Types), allowing actors to be written in any language that can target WebAssembly.

### Type-Safe WebAssembly Function Calls

The WebAssembly interface provides type safety for function calls:

```rust
// Inside wasm.rs
pub trait TypedFunction: Send + Sync + 'static {
    fn call_func<'a>(
        &'a self,
        store: &'a mut Store<ActorStore>,
        state: Option<Vec<u8>>,
        params: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(Option<Vec<u8>>, Vec<u8>)>> + Send + 'a>>;
}
```

This trait ensures that all function calls across the WebAssembly boundary are properly typed and safely handled.

### Capability Provisioning

Capabilities are provisioned according to manifest specifications:

```rust
// Inside actor_runtime.rs
fn provision_capabilities(&mut self, manifest: &ManifestConfig) -> Result<(), Error> {
    for handler_config in &manifest.handlers {
        match handler_config.handler_type {
            HandlerType::HttpServer => {
                self.add_capability(
                    HttpServerCapability::new(&handler_config.config)?
                )?;
            },
            HandlerType::FileSystem => {
                self.add_capability(
                    FileSystemCapability::new(&handler_config.config)?
                )?;
            },
            // Other capability types...
        }
    }
    Ok(())
}
```

Each capability is implemented as a module that exposes host functions to the WebAssembly environment while enforcing security and resource constraints.

### Event Chain Implementation

The Chain system uses cryptographic linking to ensure integrity:

```rust
// Inside chain.rs
fn append_event(&mut self, event: Event) -> Result<EventId, Error> {
    // Get the previous event's ID
    let prev_id = match self.events.last() {
        Some((id, _)) => *id,
        None => EventId::genesis(),
    };
    
    // Create a new event ID based on the previous ID and event content
    let id = EventId::new(&prev_id, &event);
    
    // Store the event
    self.events.push((id, event));
    self.persist()?;
    
    Ok(id)
}
```

This creates a tamper-evident history that can be verified and used for deterministic replay.

### Message Routing

The Theater Runtime implements message routing between actors:

```rust
// Inside theater_runtime.rs
fn route_message(&self, from: Option<ActorId>, to: ActorId, message: Message) -> Result<(), Error> {
    // Find the target actor
    let actor = self.actors.get(&to)?;
    
    // Record message in sender's outbox and receiver's inbox
    if let Some(sender) = from {
        let sender_actor = self.actors.get(&sender)?;
        sender_actor.record_outgoing_message(&to, &message)?;
    }
    
    // Deliver message to recipient
    actor.handle_message(from, message)
}
```

This ensures all inter-actor communication is properly tracked and delivered.

### WebAssembly Instance Management

The WebAssembly interface manages actor instances and their lifecycle:

```rust
// Inside wasm.rs
pub async fn instantiate(self) -> Result<ActorInstance> {
    let mut store = Store::new(&self.engine, self.actor_store.clone());

    let instance = self
        .linker
        .instantiate_async(&mut store, &self.component)
        .await
        .map_err(|e| WasmError::WasmError {
            context: "instantiation",
            message: e.to_string(),
        })?;

    Ok(ActorInstance {
        actor_component: self,
        instance,
        store,
        functions: HashMap::new(),
    })
}
```

This creates an isolated WebAssembly instance with its own memory space and execution context.
