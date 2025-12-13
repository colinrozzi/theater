# Theater Examples

This directory contains examples demonstrating how to use the Theater runtime with migrated handlers.

## Full Runtime Example

The `full-runtime.rs` example shows how to create a complete Theater runtime with all migrated handler crates.

### Running the Example

```bash
cargo run --example full-runtime
```

### What It Demonstrates

The example creates a Theater runtime with **ALL 11 migrated handlers**:

- ✅ **environment** - Environment variable access
- ✅ **random** - Random value generation
- ✅ **timing** - Delays and timeouts
- ✅ **runtime** - Runtime functions (log, get-state, shutdown)
- ✅ **http-client** - HTTP request capabilities
- ✅ **filesystem** - File system operations
- ✅ **process** - OS process spawning and management
- ✅ **store** - Content-addressed storage
- ✅ **supervisor** - Actor supervision
- ✅ **message-server** - Inter-actor messaging
- ✅ **http-framework** - HTTP/HTTPS server framework

**Achievement:** Through lazy initialization, all handlers can now be registered at runtime creation time! ProcessHandler stores its ActorHandle when the `start()` method is called.

### Key Code Patterns

#### 1. Creating a Handler Registry

```rust
use theater::handler::HandlerRegistry;

let mut registry = HandlerRegistry::new();
```

#### 2. Registering Handlers with Configuration

```rust
// Simple handlers with config
let env_config = EnvironmentHandlerConfig {
    allowed_vars: None,
    denied_vars: Some(vec!["AWS_SECRET_ACCESS_KEY".to_string()]),
    allow_list_all: false,
    allowed_prefixes: None,
};
registry.register(EnvironmentHandler::new(env_config, None));

// Handlers that need theater_tx channel
let runtime_config = RuntimeHostConfig {};
registry.register(RuntimeHandler::new(runtime_config, theater_tx.clone(), None));

// Handlers that use lazy initialization (ActorHandle set in start())
let process_config = ProcessHostConfig {
    max_processes: 10,
    max_output_buffer: 1024 * 1024,
    allowed_programs: None,
    allowed_paths: None,
};
registry.register(ProcessHandler::new(process_config, None));

// Framework handlers with special requirements
let message_router = theater_handler_message_server::MessageRouter::new();
registry.register(MessageServerHandler::new(None, message_router));
```

#### 3. Creating the Runtime

```rust
use theater::theater_runtime::TheaterRuntime;
use theater::chain::ChainEvent;

let (theater_tx, theater_rx) = mpsc::channel::<TheaterCommand>(32);
let (channel_events_tx, _channel_events_rx) = mpsc::channel(32);

let mut runtime: TheaterRuntime<ChainEvent> = TheaterRuntime::new(
    theater_tx.clone(),
    theater_rx,
    Some(channel_events_tx),
    handler_registry,
)
.await?;
```

#### 4. Running the Runtime

```rust
// In production, spawn this in a background task
tokio::spawn(async move {
    runtime.run().await
});

// Use theater_tx to send commands
```

### Handler Configuration

Each handler accepts:
1. **Config object** - Handler-specific configuration (may be empty struct `{}`)
2. **Permissions** - Optional permission restrictions (`None` for unrestricted)

See the example source code for detailed configuration examples for each handler.

### Production Usage

For a complete production-ready server with all 11 handlers, see:

```bash
cargo run -p theater-server-cli
```

The theater-server-cli demonstrates proper integration of all handlers in a server context.
