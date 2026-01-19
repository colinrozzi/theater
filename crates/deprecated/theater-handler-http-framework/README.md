# theater-handler-http-framework

HTTP framework handler for Theater WebAssembly actors, providing complete HTTP/HTTPS server capabilities with routing, middleware, and WebSocket support.

## Features

- **HTTP & HTTPS servers**: Create multiple HTTP or HTTPS servers with TLS support
- **Flexible routing**: Add routes with HTTP method and path patterns (powered by Axum)
- **Middleware support**: Add middleware to routes with priority-based execution
- **WebSocket support**: Enable WebSocket endpoints with connect/message/disconnect handlers
- **TLS/HTTPS**: Full TLS support with certificate and key loading
- **Multiple servers**: Create and manage multiple independent server instances
- **Event logging**: All HTTP operations are logged to the chain

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
theater-handler-http-framework = "0.2"
```

## Usage

### Creating the Handler

```rust
use theater_handler_http_framework::HttpFrameworkHandler;
use theater::config::permissions::HttpFrameworkPermissions;

// Create without permissions
let handler = HttpFrameworkHandler::new(None);

// Or with permissions
let permissions = HttpFrameworkPermissions {
    // Configure permissions as needed
};
let handler = HttpFrameworkHandler::new(Some(permissions));
```

### Host Functions (WIT Interface: `theater:simple/http-framework`)

The handler provides the following host functions for WASM actors:

#### Server Management

- **`create-server(config: server-config) -> result<u64, string>`**
  - Creates a new HTTP/HTTPS server instance
  - Returns a server ID

- **`start-server(server-id: u64) -> result<u16, string>`**
  - Starts a server on the configured port (or random port if 0)
  - Returns the actual port number

- **`stop-server(server-id: u64) -> result<_, string>`**
  - Stops a running server gracefully

- **`destroy-server(server-id: u64) -> result<_, string>`**
  - Stops and destroys a server instance

- **`get-server-info(server-id: u64) -> result<server-info, string>`**
  - Gets information about a server (port, running state, route counts, etc.)

#### Routing

- **`register-handler(name: string) -> result<u64, string>`**
  - Registers a named handler for use in routes/middleware
  - Returns a handler ID

- **`add-route(server-id: u64, path: string, method: string, handler-id: u64) -> result<u64, string>`**
  - Adds a route to a server with path pattern and HTTP method
  - Supports Axum path patterns like `/users/:id` and `/files/{*path}`
  - Returns a route ID

- **`remove-route(route-id: u64) -> result<_, string>`**
  - Removes a route from its server

#### Middleware

- **`add-middleware(server-id: u64, path: string, handler-id: u64) -> result<u64, string>`**
  - Adds middleware to a server for a specific path prefix
  - Returns a middleware ID

- **`remove-middleware(middleware-id: u64) -> result<_, string>`**
  - Removes middleware from its server

#### WebSocket

- **`enable-websocket(server-id: u64, path: string, connect-handler: option<u64>, message-handler: u64, disconnect-handler: option<u64>) -> result<_, string>`**
  - Enables WebSocket on a path with optional connect/disconnect handlers

- **`disable-websocket(server-id: u64, path: string) -> result<_, string>`**
  - Disables WebSocket on a path

- **`send-websocket-message(server-id: u64, connection-id: u64, message: websocket-message) -> result<_, string>`**
  - Sends a message to a WebSocket connection

- **`close-websocket(server-id: u64, connection-id: u64) -> result<_, string>`**
  - Closes a WebSocket connection

### Export Functions (WIT Interface: `theater:simple/http-handlers`)

WASM actors must export these functions to handle HTTP requests:

#### HTTP Handlers

```wit
handle-request: func(handler-id: u64, request: http-request) -> http-response
```

Called when an HTTP request matches a route. The actor should return an HTTP response.

```wit
handle-middleware: func(handler-id: u64, request: http-request) -> middleware-result
```

Called when middleware processes a request. Return `proceed: true` to continue, `proceed: false` to reject.

#### WebSocket Handlers

```wit
handle-websocket-connect: func(handler-id: u64, connection-id: u64, path: string, protocol: option<string>)
```

Called when a WebSocket connection is established (if connect handler is registered).

```wit
handle-websocket-message: func(handler-id: u64, connection-id: u64, message: websocket-message) -> list<websocket-message>
```

Called when a WebSocket message is received. Can return messages to send back to the client.

```wit
handle-websocket-disconnect: func(handler-id: u64, connection-id: u64)
```

Called when a WebSocket connection closes (if disconnect handler is registered).

## Architecture

### Module Structure

```
theater-handler-http-framework/
├── src/
│   ├── lib.rs              # Main handler implementation
│   ├── types.rs            # Type definitions (ServerConfig, HttpRequest, etc.)
│   ├── server_instance.rs  # Server lifecycle and routing (using Axum)
│   ├── tls.rs              # TLS certificate loading and validation
│   └── handlers.rs         # Handler registry for managing route handlers
```

### Key Components

**HttpFrameworkHandler**
- Implements the `Handler` trait
- Manages multiple server instances
- Provides host functions for WASM actors
- Handles shutdown and cleanup

**ServerInstance**
- Manages individual HTTP/HTTPS server lifecycle
- Uses Axum for routing and request handling
- Supports HTTP, HTTPS (with TLS), and WebSocket
- Graceful shutdown with connection cleanup

**TLS Support**
- Loads PEM-encoded certificates and private keys
- Validates TLS configuration before server start
- Powered by rustls

## Example

### WASM Actor Code

```rust
use theater::prelude::*;

#[export]
fn run() {
    // Create HTTP server
    let config = ServerConfig {
        port: Some(8080),
        host: Some("127.0.0.1".to_string()),
        tls_config: None,
    };

    let server_id = http_framework::create_server(config).unwrap();

    // Register a handler
    let handler_id = http_framework::register_handler("my-handler".to_string()).unwrap();

    // Add a route
    let route_id = http_framework::add_route(
        server_id,
        "/hello".to_string(),
        "GET".to_string(),
        handler_id
    ).unwrap();

    // Start the server
    let port = http_framework::start_server(server_id).unwrap();
    println!("Server listening on port {}", port);
}

#[export]
fn handle_request(handler_id: u64, request: HttpRequest) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![("content-type".to_string(), "text/plain".to_string())],
        body: Some(b"Hello, World!".to_vec()),
    }
}
```

### HTTPS Server with TLS

```rust
let config = ServerConfig {
    port: Some(8443),
    host: Some("0.0.0.0".to_string()),
    tls_config: Some(TlsConfig {
        cert_path: "/path/to/cert.pem".to_string(),
        key_path: "/path/to/key.pem".to_string(),
    }),
};

let server_id = http_framework::create_server(config).unwrap();
let port = http_framework::start_server(server_id).unwrap();
```

### WebSocket Support

```rust
// Register handlers
let connect_handler = http_framework::register_handler("ws-connect").unwrap();
let message_handler = http_framework::register_handler("ws-message").unwrap();
let disconnect_handler = http_framework::register_handler("ws-disconnect").unwrap();

// Enable WebSocket
http_framework::enable_websocket(
    server_id,
    "/ws".to_string(),
    Some(connect_handler),
    message_handler,
    Some(disconnect_handler)
).unwrap();

#[export]
fn handle_websocket_connect(handler_id: u64, connection_id: u64, path: String, protocol: Option<String>) {
    println!("WebSocket connection {} established", connection_id);
}

#[export]
fn handle_websocket_message(
    handler_id: u64,
    connection_id: u64,
    message: WebSocketMessage
) -> Vec<WebSocketMessage> {
    // Echo the message back
    vec![message]
}

#[export]
fn handle_websocket_disconnect(handler_id: u64, connection_id: u64) {
    println!("WebSocket connection {} closed", connection_id);
}
```

## Performance

The handler uses Axum's native routing which provides:
- **Fast routing**: Efficient path matching with compile-time optimizations
- **Zero-copy**: Minimal allocations for request/response handling
- **Async I/O**: Non-blocking I/O for all operations
- **Connection pooling**: Efficient resource management

## Supported HTTP Methods

- Standard: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`, `TRACE`, `CONNECT`
- WebDAV: `LOCK`, `UNLOCK`, `MKCOL`, `COPY`, `MOVE`, `PROPFIND`, `PROPPATCH`
- Wildcard: `*` (matches all methods)

## Path Pattern Syntax

The handler uses Axum's path routing syntax:

- **Static paths**: `/users`, `/api/v1/posts`
- **Named parameters**: `/users/:id`, `/posts/:post_id/comments/:comment_id`
- **Wildcard catch-all**: `/files/{*path}` (captures remaining path)

## Migration from Core Handler

This handler was migrated from `/crates/theater/src/host/framework/` to follow the new handler architecture pattern. The migration includes:

- ✅ Extracted into separate crate (`theater-handler-http-framework`)
- ✅ Implements the `Handler` trait
- ✅ Independent lifecycle management
- ✅ Per-actor handler instances via `create_instance()`
- ✅ Comprehensive event logging
- ✅ Clean shutdown handling

## Tests

Run the test suite:

```bash
cargo test
```

The test suite includes:
- Unit tests for handler creation and cloning
- TLS configuration validation tests
- Type system tests (via doc tests)

## License

Licensed under the same license as the Theater project.
