# Theater Handlers

Handlers are the primary way actors interact with the outside world and with each other in Theater. This section provides an overview and links to detailed documentation for each handler type.

## Handler System Overview

The [Handler System](handlers/README.md) documentation provides a comprehensive overview of how handlers work in Theater, including:

- What handlers are and their role in the Theater architecture
- How handlers connect actors to the outside world and with other actors
- The distinction between "host" functions (imports) and "export" functions
- The handler lifecycle within the actor runtime
- How handlers are configured in manifests

## Available Handlers

Theater provides several built-in handlers that enable different capabilities:

### [Message Server Handler](handlers/message-server.md)

The Message Server Handler is the primary mechanism for actor-to-actor communication, enabling:
- One-way message sending
- Request-response patterns
- Channel-based communication

### [HTTP Client Handler](handlers/http-client.md)

The HTTP Client Handler allows actors to make HTTP requests to external services, with:
- Support for all HTTP methods
- Header and body customization
- Automatic state chain recording

### [HTTP Framework Handler](handlers/http-framework.md)

The HTTP Framework Handler exposes actor functionality via HTTP endpoints, enabling:
- HTTP server capabilities
- RESTful API development
- Web service creation

### [Filesystem Handler](handlers/filesystem.md)

The Filesystem Handler provides actors with controlled access to the local filesystem for:
- Reading and writing files
- Directory operations
- File metadata access

### [Supervisor Handler](handlers/supervisor.md)

The Supervisor Handler enables parent-child relationships between actors, supporting:
- Spawning child actors
- Lifecycle management
- Supervision strategies

### [Store Handler](handlers/store.md)

The Store Handler provides access to Theater's content-addressable storage system for:
- Content-addressed storage
- Label management
- Persistent data storage

### [Runtime Handler](handlers/runtime.md)

The Runtime Handler provides information about and control over the actor's runtime environment:
- System information
- Environment variables
- Logging and metrics

### [Timing Handler](handlers/timing.md)

The Timing Handler provides time-related capabilities:
- Controlled delays
- Timeout patterns
- High-resolution timing

## Next Steps

Choose a handler from the list above to learn more about its capabilities, configuration options, and usage patterns.
