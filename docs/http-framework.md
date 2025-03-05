# HTTP Framework for Theater

The HTTP Framework is a powerful feature in Theater that gives WebAssembly actors full control over the lifecycle of HTTP and WebSocket servers. Instead of the traditional approach where servers are pre-configured in the actor manifest, the HTTP Framework allows actors to programmatically create, configure, and manage their own servers.

## Key Features

- **Dynamic Server Creation**: Create HTTP servers at runtime with custom configurations
- **Flexible Routing**: Add and remove routes with different HTTP methods
- **Middleware Support**: Apply middleware to specific URL paths for cross-cutting concerns
- **WebSocket Integration**: Enable WebSocket support on specific paths
- **Lifecycle Management**: Start, stop, and destroy servers as needed
- **Full Control**: Actors have complete control over their server configurations

## Getting Started

### 1. Enable the HTTP Framework

To use the HTTP Framework, add the handler to your actor's manifest file:

```toml
[[handlers]]
type = "http-framework"
config = {}
```

### 2. Create a Server

```rust
use ntwk::theater::host::http_framework;

// Create a new HTTP server
let config = http_framework::ServerConfig {
    port: Some(8080),       // Use None for auto-assigned port
    host: Some("0.0.0.0".to_string()),
    tls_config: None,       // Optional TLS config
};

let server_id = http_framework::create_server(config)?;
```

### 3. Register Handlers

```rust
// Register handlers with unique names
let api_handler_id = http_framework::register_handler("handle_api")?;
let auth_middleware_id = http_framework::register_handler("auth_middleware")?;
let ws_handler_id = http_framework::register_handler("handle_websocket")?;
```

### 4. Add Routes and Middleware

```rust
// Add routes with specific HTTP methods
http_framework::add_route(server_id, "/api/data", "GET", api_handler_id)?;
http_framework::add_route(server_id, "/api/data", "POST", api_handler_id)?;

// Add middleware
http_framework::add_middleware(server_id, "/api", auth_middleware_id)?;
```

### 5. Enable WebSocket (Optional)

```rust
// Enable WebSocket support on a path
http_framework::enable_websocket(
    server_id,
    "/ws",
    Some(connect_handler_id),    // Optional connect handler
    message_handler_id,          // Required message handler
    Some(disconnect_handler_id), // Optional disconnect handler
)?;
```

### 6. Start the Server

```rust
// Start the server (returns the actual port)
let actual_port = http_framework::start_server(server_id)?;
println!("Server started on port {}", actual_port);
```

## Handler Implementation

Here's how to implement the handler functions:

### HTTP Request Handler

```rust
// Handler for HTTP requests
#[no_mangle]
pub fn handle_api(handler_id: u64, request: HttpRequest) -> Result<HttpResponse, String> {
    // Process the request
    let response = HttpResponse {
        status: 200,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: Some("{'success': true}".as_bytes().to_vec()),
    };
    
    Ok(response)
}
```

### Middleware Handler

```rust
// Middleware handler
#[no_mangle]
pub fn auth_middleware(handler_id: u64, request: HttpRequest) -> Result<MiddlewareResult, String> {
    // Check for authorization header
    let auth_header = request.headers.iter().find(|(name, _)| name == "authorization");
    
    if auth_header.is_none() {
        // Reject the request
        return Ok(MiddlewareResult {
            proceed: false,
            request: request,
        });
    }
    
    // Allow the request to proceed
    Ok(MiddlewareResult {
        proceed: true,
        request: request,
    })
}
```

### WebSocket Handlers

```rust
// WebSocket connect handler
#[no_mangle]
pub fn handle_ws_connect(handler_id: u64, connection_id: u64, path: String, query: Option<String>) -> Result<(), String> {
    println!("New WebSocket connection: {}", connection_id);
    Ok(())
}

// WebSocket message handler
#[no_mangle]
pub fn handle_ws_message(handler_id: u64, connection_id: u64, message: WebSocketMessage) -> Result<Vec<WebSocketMessage>, String> {
    // Echo the message back
    Ok(vec![message])
}

// WebSocket disconnect handler
#[no_mangle]
pub fn handle_ws_disconnect(handler_id: u64, connection_id: u64) -> Result<(), String> {
    println!("WebSocket disconnected: {}", connection_id);
    Ok(())
}
```

## Server Lifecycle Management

```rust
// Get server info
let server_info = http_framework::get_server_info(server_id)?;

// Stop a server (can be restarted later)
http_framework::stop_server(server_id)?;

// Destroy a server (permanently removes it)
http_framework::destroy_server(server_id)?;
```

## WebSocket Message Sending

You can send messages to connected WebSocket clients:

```rust
// Send a message to a specific connection
let message = WebSocketMessage {
    ty: MessageType::Text,
    data: None,
    text: Some("Hello from server!".to_string()),
};

http_framework::send_websocket_message(server_id, connection_id, message)?;

// Close a specific connection
http_framework::close_websocket(server_id, connection_id)?;
```

## Events and Logging

All HTTP Framework operations are recorded in the actor's event chain, providing a complete audit trail of server lifecycle events, requests, and responses.

## Comparison with Traditional HTTP Handlers

| Feature | HTTP Framework | Traditional Handlers |
|---------|---------------|----------------------|
| Server Creation | Dynamic at runtime | Static in manifest |
| Port Selection | Fixed or auto-assigned | Fixed in manifest |
| Multiple Servers | Yes, unlimited | One per handler type |
| Server Lifecycle | Full control | Fixed for actor lifetime |
| Routing | Dynamic, method-specific | Limited by implementation |
| Middleware | Yes, with path filtering | No |
| WebSocket Integration | Built-in with handler callbacks | Separate handler |
