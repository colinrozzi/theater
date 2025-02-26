# Theater Handler System

Handlers are the primary way actors interact with the outside world and with each other in Theater. Each handler type provides specific capabilities while maintaining the state tracking and verification that are core to Theater's design.

## Current Handler Implementation

The handler system is implemented in Theater's core and configured through actor manifests. The current implementation provides several handler types:

### Message Server Handler

The message server handler enables actor-to-actor communication:

```toml
[[handlers]]
type = "message-server"
config = {}
interface = "ntwk:theater/message-server-client"
```

This handler:
- Enables actors to receive messages from other actors
- Supports both one-way (send) and request-response patterns
- Uses JSON serialized as bytes for message interchange
- Records all interactions in the state chain

The handler works with two functions from the message-server-client interface:

```wit
handle-send: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>>, string>;
handle-request: func(state: option<json>, params: tuple<json>) -> result<tuple<option<json>, tuple<json>>, string>;
```

### HTTP Server Handler

The HTTP server handler exposes actor functionality via HTTP:

```toml
[[handlers]]
type = "http-server"
config = { port = 8080 }
```

This handler:
- Creates an HTTP server on the specified port
- Routes HTTP requests to the actor
- Converts HTTP requests/responses to/from actor messages
- Records all interactions in the state chain

The handler works with the http-server interface:

```wit
handle-request: func(state: state, params: tuple<http-request>) -> result<tuple<state, tuple<http-response>>, string>;
```

### Supervisor Handler

The supervisor handler enables parent-child actor relationships:

```toml
[[handlers]]
type = "supervisor"
config = {}
```

This handler:
- Allows the actor to spawn and manage child actors
- Provides access to child state and events
- Enables the supervision tree pattern
- Records all supervision actions in the state chain

The handler implements the supervisor interface (see `supervisor.wit`).

## Handler Configuration

Handlers are configured in the actor's manifest file (TOML format):

```toml
name = "my-actor"
component_path = "my_actor.wasm"

[[handlers]]
type = "message-server"
config = {}

[[handlers]]
type = "http-server"
config = { port = 8080 }

[[handlers]]
type = "supervisor"
config = {}
```

Each handler has:
- `type`: The handler type identifier
- `config`: Handler-specific configuration
- Optional `interface`: The WIT interface this handler implements

## Handler Registration and Initialization

When an actor is loaded, Theater:

1. Reads the manifest configuration
2. For each handler:
   - Creates the handler instance
   - Configures it with the specified options
   - Registers it with the actor runtime
   - Sets up any necessary resources (sockets, etc.)

Handler initialization is part of the actor initialization process:

```rust
// From actor_runtime.rs
pub async fn start(
    actor_id: TheaterId,
    manifest: &ManifestConfig,
    runtime_tx: mpsc::Sender<TheaterCommand>,
    mailbox_rx: mpsc::Receiver<ActorMessage>,
) -> Result<()> {
    // Initialize actor instance
    let instance = ActorInstance::new(actor_id.clone(), manifest).await?;
    
    // Initialize handlers from manifest
    for handler_config in &manifest.handlers {
        match handler_config.type_.as_str() {
            "message-server" => {
                // Initialize message server handler
                // ...
            },
            "http-server" => {
                // Initialize HTTP server handler
                // ...
            },
            "supervisor" => {
                // Initialize supervisor handler
                // ...
            },
            _ => {
                return Err(anyhow::anyhow!("Unknown handler type"));
            }
        }
    }
    
    // Start actor runtime
    // ...
}
```

## Message Flow Through Handlers

### Message Server Handler Flow

1. Incoming message arrives via ActorMessage
2. Runtime routes message to message-server handler
3. Handler extracts message data
4. Handler calls actor's handle-send or handle-request function
5. Actor processes message and returns new state (and response for requests)
6. State change is recorded in hash chain
7. Response (if any) is returned to sender

### HTTP Server Handler Flow

1. HTTP request arrives at the server
2. Server converts request to http-request format
3. Handler calls actor's handle-request function
4. Actor processes request and returns new state and response
5. State change is recorded in hash chain
6. Response is converted back to HTTP and sent to client

### Supervisor Handler Flow

1. Actor invokes a supervisor function
2. Handler converts call to appropriate TheaterCommand
3. Command is sent to TheaterRuntime
4. Runtime performs the requested operation
5. Result is returned to actor
6. State changes are recorded in hash chain

## Handler Implementation Details

### Message Server Handler

```rust
// Pseudo-implementation
pub struct MessageServerHandler {
    instance: ActorInstance,
}

impl MessageServerHandler {
    pub async fn new(instance: ActorInstance) -> Self {
        Self { instance }
    }
    
    pub async fn handle_send(&self, data: Vec<u8>) -> Result<()> {
        // Get current state
        let state = self.instance.store.data().get_state();
        
        // Call actor's handle-send function
        let (new_state, _) = self.instance
            .call_function("handle-send", state, data)
            .await?;
        
        // Update state
        self.instance.store.data_mut().set_state(new_state);
        Ok(())
    }
    
    pub async fn handle_request(&self, data: Vec<u8>) -> Result<Vec<u8>> {
        // Get current state
        let state = self.instance.store.data().get_state();
        
        // Call actor's handle-request function
        let (new_state, response) = self.instance
            .call_function("handle-request", state, data)
            .await?;
        
        // Update state
        self.instance.store.data_mut().set_state(new_state);
        Ok(response)
    }
}
```

### HTTP Server Handler

```rust
// Pseudo-implementation
pub struct HttpServerHandler {
    instance: ActorInstance,
    port: u16,
    server: Option<Server>,
}

impl HttpServerHandler {
    pub async fn new(instance: ActorInstance, config: HttpServerConfig) -> Self {
        Self {
            instance,
            port: config.port,
            server: None,
        }
    }
    
    pub async fn start(&mut self) -> Result<()> {
        // Create HTTP server
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        
        // Set up routes
        let instance = self.instance.clone();
        let app = Router::new()
            .route("/*path", get(move |req| handle_request(instance.clone(), req)))
            .route("/*path", post(move |req| handle_request(instance.clone(), req)))
            // Add other HTTP methods...
            
        // Start server
        self.server = Some(axum::Server::bind(&addr)
            .serve(app.into_make_service()));
            
        Ok(())
    }
    
    async fn handle_request(instance: ActorInstance, req: Request) -> Response {
        // Convert HTTP request to WIT http-request
        let wit_req = convert_request(req);
        
        // Get current state
        let state = instance.store.data().get_state();
        
        // Call actor's handle-request function
        let (new_state, wit_resp) = instance
            .call_function("handle-request", state, wit_req)
            .await?;
            
        // Update state
        instance.store.data_mut().set_state(new_state);
        
        // Convert WIT response to HTTP response
        convert_response(wit_resp)
    }
}
```

## Custom Handler Development

To develop a new handler for Theater:

1. **Define the WIT Interface**:
   - Create a new `.wit` file defining the interface
   - Specify functions and types
   - Document behavior

2. **Implement the Handler in Theater**:
   - Add handler type to configuration parser
   - Create handler implementation
   - Connect to actor runtime

3. **Add Handler Registration**:
   - Update actor runtime to recognize new handler
   - Implement handler initialization
   - Connect to WIT interface

## Best Practices

### Handler Configuration

1. **Port Configuration**:
   - Use different ports for different handlers
   - Consider configurable port ranges
   - Document port usage

2. **Resource Limits**:
   - Specify resource limits in configuration
   - Consider memory and connection limits
   - Set appropriate timeouts

3. **Interface Versions**:
   - Specify interface versions explicitly
   - Maintain backward compatibility
   - Document breaking changes

### Handler Usage

1. **Message Patterns**:
   - Use clear message types
   - Implement consistent error handling
   - Document message formats

2. **HTTP Endpoints**:
   - Follow RESTful conventions
   - Use appropriate status codes
   - Document API behavior

3. **Supervision Patterns**:
   - Design clear supervision hierarchies
   - Define restart strategies
   - Document parent-child relationships

## Security Considerations

1. **Input Validation**:
   - Validate all incoming messages
   - Sanitize HTTP input
   - Check message size and format

2. **Resource Protection**:
   - Implement rate limiting
   - Protect against DoS attacks
   - Monitor resource usage

3. **Access Control**:
   - Consider authentication for handlers
   - Implement authorization checks
   - Document security requirements
