# Handler System

The handler system is the core of how actors interact with the outside world and with each other in Theater. Handlers provide the bridge between WebAssembly actors and host capabilities, enabling actors to send messages, access resources, and participate in supervision hierarchies.

## What Are Handlers?

Handlers are specialized components that:

1. **Connect Actors with Host Capabilities**: Handlers expose host functions to WebAssembly actors, allowing them to interact with the host system and other actors
2. **Process Messages and Events**: Handlers receive and process incoming messages, translating them into actor function calls
3. **Maintain Chain Integrity**: All handler operations are recorded in the actor's chain, maintaining the verifiable history
4. **Provide Standard Interfaces**: Handlers implement standard WebAssembly Interface Type (WIT) interfaces, ensuring consistency across actors

## Handler Architecture

Each handler consists of two main parts:

1. **Host Functions** (Imports): Functions provided by the host environment that actors can call. These are integrated into the actor's WebAssembly module during instantiation.

2. **Exported Functions** (Exports): Functions that the actor implements and the handler calls in response to external events or messages.

This bidirectional interface allows for complete interaction patterns while maintaining the security boundaries provided by WebAssembly.

## Handler Lifecycle

When an actor is started:

1. **Registration**: Handlers specified in the actor's manifest are registered with the actor runtime
2. **Initialization**: Each handler is initialized and connected to the actor component
3. **Host Function Setup**: The handler adds its host functions to the actor's WebAssembly linker
4. **Export Function Registration**: The handler registers the actor's exported functions for callbacks
5. **Start**: The handler's event loop is started in a separate task

During operation:

1. **Message Processing**: Handlers receive messages or events and process them
2. **Function Calls**: Handlers call actor functions or respond to actor requests to host functions
3. **State Recording**: All interactions are recorded in the actor's state chain

During shutdown:

1. **Graceful Termination**: Handlers receive shutdown signals and perform cleanup
2. **Resource Release**: All resources owned by handlers are released

## Handler Configuration

Handlers are configured in the actor's manifest file (TOML format):

```toml
name = "my-actor"
component_path = "my_actor.wasm"

[[handlers]]
type = "message-server"
config = {}

[[handlers]]
type = "http-client"
config = {}

[[handlers]]
type = "filesystem"
config = { path = "data", new_dir = true }
```

Each handler entry includes:
- `type`: The handler type identifier
- `config`: Handler-specific configuration options

## Available Handler Types

Theater provides several built-in handler types:

1. **message-server**: Enables actor-to-actor messaging using both synchronous (request/response) and asynchronous (fire-and-forget) patterns
2. **http-client**: Allows actors to make HTTP requests to external services
3. **http-framework**: Exposes actor functionality via HTTP endpoints
4. **filesystem**: Provides access to the filesystem with appropriate sandboxing
5. **supervisor**: Enables parent-child actor relationships for supervision
6. **store**: Provides content-addressable storage for actors
7. **runtime**: Gives access to runtime information and operations
8. **timing**: Provides timing and scheduling capabilities

## WebAssembly Interface Types (WIT)

Handler abilitiesare exposed to the actors using WebAssembly Interface Types (WIT), which provide a language-agnostic way to describe component interfaces. For example, the message-server interface is defined as:

```wit
interface message-server-client {
    use types.{json, event};

    handle-send: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>>, string>;
    handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
}

interface message-server-host {
    use types.{json, actor-id};

    send: func(actor-id: actor-id, msg: json) -> result<_, string>;
    request: func(actor-id: actor-id, msg: json) -> result<json, string>;
}
```

## Handler Implementation Details

Under the hood, handlers are implemented as Rust structs that:

1. Implement the `Handler` trait
2. Handle setup of host functions
3. Process messages and events
4. Call actor functions when needed
5. Maintain appropriate state

For example, the `MessageServerHost` implements handler functionality for actor-to-actor messaging.

## Handler Security Model

The handler system is designed with security in mind:

1. **Sandboxed Access**: Handlers provide controlled access to host resources
2. **Verifiable State**: All handler operations are recorded in the chain
3. **Explicit Permissions**: Actors must explicitly declare which handlers they use

## Developing Custom Handlers

To develop a new handler for Theater:

1. **Define the WIT Interface**: Create a new `.wit` file defining the interface
2. **Implement the Handler**: Create a new handler implementation in Rust
3. **Register the Handler**: Add the handler to the configuration system
4. **Connect to Actor Runtime**: Integrate the handler with the actor runtime

## Best Practices

1. **Handler Selection**: Only include handlers that your actor actually needs
2. **Resource Management**: Configure appropriate resource limits for handlers
3. **Error Handling**: Implement proper error handling for handler operations
4. **Testing**: Test handlers in isolation before integrating them

## Next Steps

In the following sections, we'll explore each handler type in detail, including:
- Specific configuration options
- Available functions
- Usage patterns
- Examples

See the individual handler documentation for more details:
- [Message Server Handler](message-server.md)
- [HTTP Client Handler](http-client.md)
- [HTTP Framework Handler](http-framework.md)
- [File System Handler](filesystem.md)
- [Supervisor Handler](supervisor.md)
- [Store Handler](store.md)
- [Runtime Handler](runtime.md)
- [Timing Handler](timing.md)
