# HTTP Framework for Theater

The HTTP Framework is a powerful feature in Theater that gives WebAssembly actors full control over the lifecycle of HTTP and WebSocket servers. Instead of the traditional approach where servers are pre-configured in the actor manifest, the HTTP Framework allows actors to programmatically create, configure, and manage their own servers.

## Key Features

- **Dynamic Server Creation**: Create HTTP servers at runtime with custom configurations
- **Flexible Routing**: Add and remove routes with different HTTP methods
- **Middleware Support**: Apply middleware to specific URL paths for cross-cutting concerns
- **WebSocket Integration**: Enable WebSocket support on specific paths
- **Lifecycle Management**: Start, stop, and destroy servers as needed
- **Full Control**: Actors have complete control over their server configurations
- **State Management**: Consistent state handling across all HTTP and WebSocket handlers

## Getting Started

### 1. Enable the HTTP Framework

To use the HTTP Framework, add the handler to your actor's manifest file:

```toml
[[handlers]]
type = "http-framework"
config = {}
```

### 2. Implement Required Interfaces

In your `world.wit` file, make sure to import the HTTP framework and export the handlers:

```wit
world single-actor {
    import runtime;
    // ... other imports
    import http-framework;
    export http-handlers;
    export actor;
    // ... other exports
}
```

### 3. Create a Server

In your actor's initialization function, set up the HTTP server:

```rust
use bindings::ntwk::theater::http_framework::{
    add_middleware, add_route, create_server, enable_websocket,
    register_handler, start_server, ServerConfig
};

// Setup function called from init
fn setup_http_server() -> Result<u64, String> {
    // Create server configuration
    let config = ServerConfig {
        port: Some(8080),
        host: Some("0.0.0.0".to_string()),
        tls_config: None,
    };

    // Create a new HTTP server
    let server_id = create_server(&config)?;

    // Register handlers
    let api_handler_id = register_handler("handle_api")?;
    let middleware_handler_id = register_handler("auth_middleware")?;
    let ws_handler_id = register_handler("handle_websocket")?;

    // Add middleware
    add_middleware(server_id, "/api", middleware_handler_id)?;

    // Add routes
    add_route(server_id, "/api/data", "GET", api_handler_id)?;
    add_route(server_id, "/api/data", "POST", api_handler_id)?;

    // Enable WebSocket
    enable_websocket(
        server_id,
        "/ws",
        Some(ws_handler_id), // Connect handler
        ws_handler_id,       // Message handler
        Some(ws_handler_id), // Disconnect handler
    )?;

    // Start the server
    let port = start_server(server_id)?;

    Ok(server_id)
}
```

## Handler Implementation

Implement the `HttpHandlers` trait for your actor component:

```rust
use bindings::exports::ntwk::theater::http_handlers::Guest as HttpHandlers;
use bindings::ntwk::theater::http_types::{HttpRequest, HttpResponse, MiddlewareResult};
use bindings::ntwk::theater::websocket_types::{WebsocketMessage};
use bindings::ntwk::theater::types::State;

struct Actor;

impl HttpHandlers for Actor {
    // HTTP Request Handler
    fn handle_request(
        state: State,
        params: (u64, HttpRequest),
    ) -> Result<(State, (HttpResponse,)), String> {
        let (handler_id, request) = params;
        
        // Parse the current state
        let state_bytes = state.unwrap_or_default();
        let mut app_state: AppState = if !state_bytes.is_empty() {
            serde_json::from_slice(&state_bytes).map_err(|e| e.to_string())?
        } else {
            AppState::default()
        };
        
        // Process the request based on path and method
        let response = match (request.uri.as_str(), request.method.as_str()) {
            ("/api/data", "GET") => {
                // Return data
                let data = serde_json::json!({ "data": app_state.some_field });
                let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
                
                HttpResponse {
                    status: 200,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: Some(body),
                }
            },
            // Handle other routes...
            _ => {
                // Not found
                HttpResponse {
                    status: 404,
                    headers: vec![("content-type".to_string(), "text/plain".to_string())],
                    body: Some("Not Found".as_bytes().to_vec()),
                }
            }
        };
        
        // Save updated state
        let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
        
        Ok((Some(updated_state_bytes), (response,)))
    }
    
    // Middleware Handler
    fn handle_middleware(
        state: State,
        params: (u64, HttpRequest),
    ) -> Result<(State, (MiddlewareResult,)), String> {
        let (handler_id, request) = params;
        
        // Check for authentication
        let auth_header = request
            .headers
            .iter()
            .find(|(name, _)| name.to_lowercase() == "x-api-key");
            
        if let Some((_, value)) = auth_header {
            if value == "valid-api-key" {
                // Allow request to proceed
                Ok((state, (MiddlewareResult {
                    proceed: true,
                    request,
                },)))
            } else {
                // Invalid key
                Ok((state, (MiddlewareResult {
                    proceed: false,
                    request,
                },)))
            }
        } else {
            // No key provided
            Ok((state, (MiddlewareResult {
                proceed: false,
                request,
            },)))
        }
    }
    
    // WebSocket Connect Handler
    fn handle_websocket_connect(
        state: State,
        params: (u64, u64, String, Option<String>),
    ) -> Result<(State,), String> {
        let (handler_id, connection_id, path, query) = params;
        // Process new connection
        Ok((state,))
    }
    
    // WebSocket Message Handler
    fn handle_websocket_message(
        state: State,
        params: (u64, u64, WebsocketMessage),
    ) -> Result<(State, (Vec<WebsocketMessage>,)), String> {
        let (handler_id, connection_id, message) = params;
        
        // Process message and generate responses
        let responses = vec![/* response messages */];
        
        Ok((state, (responses,)))
    }
    
    // WebSocket Disconnect Handler
    fn handle_websocket_disconnect(
        state: State,
        params: (u64, u64),
    ) -> Result<(State,), String> {
        let (handler_id, connection_id) = params;
        // Handle disconnect
        Ok((state,))
    }
}
```

## Server Lifecycle Management

You can manage your HTTP server throughout your actor's lifecycle:

```rust
use bindings::ntwk::theater::http_framework::{get_server_info, stop_server, destroy_server};

// Get server info
let server_info = get_server_info(server_id)?;

// Stop a server (can be restarted later)
stop_server(server_id)?;

// Destroy a server (permanently removes it)
destroy_server(server_id)?;
```

## WebSocket Message Sending

Send messages to connected WebSocket clients:

```rust
use bindings::ntwk::theater::http_framework::{send_websocket_message, close_websocket};
use bindings::ntwk::theater::websocket_types::{WebsocketMessage, MessageType};

// Send a message to a specific connection
let message = WebsocketMessage {
    ty: MessageType::Text,
    data: None,
    text: Some("Hello from server!".to_string()),
};

send_websocket_message(server_id, connection_id, message)?;

// Close a specific connection
close_websocket(server_id, connection_id)?;
```

## State Management

All HTTP Framework handlers receive and return the actor state. This allows consistent state management across all handler types:

1. The state is provided as the first parameter to all handlers
2. Updated state is returned as part of the result tuple
3. State is typically serialized/deserialized using serde

```rust
// Example state struct
#[derive(Serialize, Deserialize)]
struct AppState {
    count: u32,
    messages: Vec<String>,
}

// State handling in handlers
fn handle_request(
    state: State,  // Input state (Option<Vec<u8>>)
    params: (u64, HttpRequest),
) -> Result<(State, (HttpResponse,)), String> {
    // Deserialize state
    let state_bytes = state.unwrap_or_default();
    let mut app_state: AppState = if !state_bytes.is_empty() {
        serde_json::from_slice(&state_bytes).map_err(|e| e.to_string())?
    } else {
        AppState::default()
    };
    
    // Update state
    app_state.count += 1;
    
    // Serialize updated state
    let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
    
    // Return updated state and response
    Ok((Some(updated_state_bytes), (response,)))
}
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
| State Management | Consistent across all handlers | Different for each handler type |
