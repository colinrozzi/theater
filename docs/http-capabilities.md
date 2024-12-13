# HTTP Capabilities in Theater

This document describes the current HTTP capabilities available in Theater, including both actor-to-actor communication and external HTTP request handling.

## Overview

Theater provides two types of HTTP-related functionality:
1. Actor-to-actor communication (via `http.rs`)
2. External HTTP request handling (via `http_server.rs`)

## Actor-to-Actor Communication

The `http.rs` module enables actors to communicate with each other using HTTP as the transport mechanism.

### Configuration

In your actor's manifest:
```toml
[[handlers]]
type = "Http"
config = { port = 8080 }
```

### Message Format

Messages between actors are sent as JSON:

```rust
// Sending a message
let message = serde_json::json!({
    "action": "some_action",
    "data": {
        // message contents
    }
});

// Message will be sent as POST request with JSON body
```

### Implementation Example

```rust
use serde_json::Value;

// Receiving messages
fn handle(msg: Vec<u8>, state: Vec<u8>) -> Vec<u8> {
    let msg_json: Value = serde_json::from_slice(&msg)
        .expect("Invalid message format");
        
    match msg_json.get("action").and_then(|a| a.as_str()) {
        Some("some_action") => {
            // Handle the action
        },
        _ => {
            // Handle unknown action
        }
    }
    
    // Return updated state
    state
}
```

## External HTTP Request Handling

The `http_server.rs` module handles HTTP requests from external clients.

### Configuration

In your actor's manifest:
```toml
[[handlers]]
type = "Http-server"
config = { port = 8081 }
```

### Request Format

Requests are passed to actors in the following format:

```rust
ActorInput::HttpRequest {
    method: String,      // HTTP method as string
    uri: String,         // Request path
    headers: Vec<(String, String)>,  // Request headers
    body: Option<Vec<u8>>,  // Request body as bytes
}
```

### Response Format

Actors should return responses in this format:

```rust
ActorOutput::HttpResponse {
    status: u16,         // HTTP status code
    headers: Vec<(String, String)>,  // Response headers
    body: Option<Vec<u8>>,  // Response body as bytes
}
```

### Implementation Example

```rust
fn handle_request(input: ActorInput, state: Vec<u8>) -> (ActorOutput, Vec<u8>) {
    match input {
        ActorInput::HttpRequest { method, uri, headers, body } => {
            // Parse body if present
            let body_json: Value = if let Some(bytes) = body {
                serde_json::from_slice(&bytes).unwrap_or_default()
            } else {
                Value::Null
            };
            
            // Create response
            let response = ActorOutput::HttpResponse {
                status: 200,
                headers: vec![
                    ("Content-Type".to_string(), "application/json".to_string())
                ],
                body: Some(serde_json::to_vec(&response_data).unwrap()),
            };
            
            (response, state)
        },
        _ => {
            // Handle other input types
            (ActorOutput::HttpResponse {
                status: 400,
                headers: vec![],
                body: None,
            }, state)
        }
    }
}
```

## Common Patterns

### JSON Request Handling
```rust
fn parse_json_body(body: Option<Vec<u8>>) -> Result<Value, String> {
    match body {
        Some(bytes) => {
            serde_json::from_slice(&bytes)
                .map_err(|e| format!("Invalid JSON: {}", e))
        },
        None => Ok(Value::Null)
    }
}
```

### Basic Error Response
```rust
fn error_response(status: u16, message: &str) -> ActorOutput {
    ActorOutput::HttpResponse {
        status,
        headers: vec![
            ("Content-Type".to_string(), "application/json".to_string())
        ],
        body: Some(serde_json::to_vec(&serde_json::json!({
            "error": message
        })).unwrap()),
    }
}
```

## Limitations

Current limitations include:
1. No built-in routing system
2. Manual JSON parsing required
3. No middleware support
4. Basic request/response type system
5. No WebSocket support

See the change requests directory for proposed improvements to these areas.