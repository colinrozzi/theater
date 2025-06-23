
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Template metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub files: HashMap<&'static str, &'static str>,
}

/// Available templates for creating new actors
pub fn available_templates() -> HashMap<String, Template> {
    let mut templates = HashMap::new();

    // Basic actor template
    templates.insert(
        "basic".to_string(),
        Template {
            name: "basic".to_string(),
            description: "A simple Theater actor with basic functionality".to_string(),
            files: basic_template_files(),
        },
    );

    // HTTP actor template
    templates.insert(
        "http".to_string(),
        Template {
            name: "http".to_string(),
            description: "A Theater actor with HTTP server capabilities".to_string(),
            files: http_template_files(),
        },
    );

    templates
}

/// Basic actor template files
fn basic_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();

    // Add Cargo.toml
    files.insert("Cargo.toml", r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen-rt = { version = "0.39.0", features = ["bitflags"] }
"#);

    // Add manifest.toml
    files.insert("manifest.toml", r#"name = "{{project_name}}"
version = "0.1.0"
description = "A basic Theater actor"
component_path = "not yet build"
save_chain = true

[interface]
implements = "theater:simple/actor"
requires = []

[[handlers]]
type = "runtime"
config = {}
"#);

    // Add src/lib.rs
    files.insert("src/lib.rs", r#"mod bindings;

use crate::bindings::exports::ntwk::theater::actor::Guest;
use crate::bindings::ntwk::theater::runtime::{log, shutdown};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct State {
    messages: Vec<String>,
}

struct Actor;
impl Guest for Actor {
    fn init(
        _init_state_bytes: Option<Vec<u8>>,
        params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        log("Initializing {{project_name}} actor");
        let (self_id,) = params;
        log(&format!("Actor ID: {}", &self_id));
        log("Hello from {{project_name}} actor!");

        shutdown("{{project_name}} actor shutting down");

        Ok((None,))
    }
}

bindings::export!(Actor with_types_in bindings);
"#);

    // Add README.md
    files.insert("README.md", r#"# {{project_name}}

A basic Theater actor created from the template.

## Building

To build the actor:

```bash
theater build
```

## Running

To run the actor with Theater:

```bash
theater start manifest.toml
```

## Features

This basic actor supports:

- Storing and retrieving state
- Handling simple messages
- Incrementing a counter
- Storing text messages

## API

You can interact with this actor using the following commands:

- `count` - Get the current count
- `messages` - Get all stored messages
- `increment` - Increment the counter
- Any other text - Store as a message

## Example

```bash
# Send a request to get the current count
theater message {{project_name}} count
```
"#);

    files
}

/// HTTP actor template files
fn http_template_files() -> HashMap<&'static str, &'static str> {
    let mut files = HashMap::new();

    // Add Cargo.toml
    files.insert("Cargo.toml", r#"[package]
name = "{{project_name}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
wit-bindgen-rt = { version = "0.39.0", features = ["bitflags"] }
"#);

    // Add manifest.toml
    files.insert("manifest.toml", r#"name = "{{project_name}}"
version = "0.1.0"
description = "An HTTP server Theater actor"
component_path = "target/wasm32-unknown-unknown/release/{{project_name_snake}}.wasm"

[interface]
implements = "theater:simple/actor"
requires = []

[[handlers]]
type = "runtime"
config = {}

[[handlers]]
type = "http-framework"
config = {}
"#);

    // Add src/lib.rs
    files.insert("src/lib.rs", r#"mod bindings;

use crate::bindings::exports::ntwk::theater::actor::Guest;
use crate::bindings::exports::ntwk::theater::http_handlers::Guest as HttpHandlers;
use crate::bindings::exports::ntwk::theater::message_server_client::Guest as MessageServerClient;
use crate::bindings::ntwk::theater::http_framework::{
    add_middleware, add_route, create_server, enable_websocket, register_handler, start_server,
    ServerConfig,
};
use crate::bindings::ntwk::theater::http_types::{HttpRequest, HttpResponse, MiddlewareResult};
use crate::bindings::ntwk::theater::runtime::log;
use crate::bindings::ntwk::theater::types::State;
use crate::bindings::ntwk::theater::websocket_types::{MessageType, WebsocketMessage};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct AppState {
    count: u32,
    messages: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            count: 0,
            messages: Vec::new(),
        }
    }
}

struct Actor;
impl Guest for Actor {
    fn init(_state: State, params: (String,)) -> Result<(State,), String> {
        log("Initializing HTTP Actor");
        let (param,) = params;
        log(&format!("Init parameter: {}", param));

        let app_state = AppState::default();
        log("Created default app state");
        let state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;

        // Create the initial state
        let new_state = Some(state_bytes);

        // Set up the HTTP server
        log("Setting up HTTP server...");
        setup_http_server().map_err(|e| e.to_string())?;
        log("HTTP server set up successfully");

        Ok((new_state,))
    }
}

// Setup the HTTP server and return the server ID
fn setup_http_server() -> Result<u64, String> {
    log("Setting up HTTP server");

    // Create server configuration
    let config = ServerConfig {
        port: Some(8080),
        host: Some("0.0.0.0".to_string()),
        tls_config: None,
    };

    // Create a new HTTP server
    let server_id = create_server(&config)?;
    log(&format!("Created server with ID: {}", server_id));

    // Register handlers
    let api_handler_id = register_handler("handle_api")?;
    let middleware_handler_id = register_handler("auth_middleware")?;
    let ws_handler_id = register_handler("handle_websocket")?;

    log(&format!(
        "Registered handlers - API: {}, Middleware: {}, WebSocket: {}",
        api_handler_id, middleware_handler_id, ws_handler_id
    ));

    // Add middleware
    add_middleware(server_id, "/api", middleware_handler_id)?;

    // Add routes
    add_route(server_id, "/api/count", "GET", api_handler_id)?;
    add_route(server_id, "/api/count", "POST", api_handler_id)?;
    add_route(server_id, "/api/messages", "GET", api_handler_id)?;
    add_route(server_id, "/api/messages", "POST", api_handler_id)?;

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
    log(&format!("Server started on port {}", port));

    Ok(server_id)
}

impl HttpHandlers for Actor {
    // HTTP Request Handler
    fn handle_request(
        state: State,
        params: (u64, HttpRequest),
    ) -> Result<(State, (HttpResponse,)), String> {
        let (handler_id, request) = params;
        log(&format!(
            "Handling HTTP request with handler ID: {}",
            handler_id
        ));
        log(&format!(
            "  Method: {}, Path: {}",
            request.method, request.uri
        ));

        // Parse the current state
        let state_bytes = state.unwrap_or_default();
        let mut app_state: AppState = if !state_bytes.is_empty() {
            log("Deserializing existing state");
            let app_state: AppState =
                serde_json::from_slice(&state_bytes).map_err(|e| e.to_string())?;
            log(&format!(
                "Current state: count={}, messages={}",
                app_state.count,
                app_state.messages.len()
            ));
            app_state
        } else {
            log("Creating default state");
            AppState::default()
        };

        // Process the request based on the path and method
        log(&format!(
            "Processing request: {} {}",
            request.method, request.uri
        ));
        let response = match (request.uri.as_str(), request.method.as_str()) {
            ("/api/count", "GET") => {
                log("Handling GET /api/count");
                // Return the current count
                let data = serde_json::json!({ "count": app_state.count });
                let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
                log(&format!("Returning count: {}", app_state.count));

                HttpResponse {
                    status: 200,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: Some(body),
                }
            }
            ("/api/count", "POST") => {
                log("Handling POST /api/count");
                // Increment the count
                app_state.count += 1;
                log(&format!("Incremented count to: {}", app_state.count));

                // Return the new count
                let data = serde_json::json!({ "count": app_state.count });
                let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;

                HttpResponse {
                    status: 200,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: Some(body),
                }
            }
            ("/api/messages", "GET") => {
                log("Handling GET /api/messages");
                // Return all messages
                let data = serde_json::json!({ "messages": app_state.messages });
                let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
                log(&format!("Returning {} messages", app_state.messages.len()));

                HttpResponse {
                    status: 200,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: Some(body),
                }
            }
            ("/api/messages", "POST") => {
                log("Handling POST /api/messages");
                // Parse the message from the request body
                if let Some(body) = &request.body {
                    log(&format!("Received request body of {} bytes", body.len()));
                    // Attempt to parse the body as a JSON object with a message field
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) {
                        log("Successfully parsed JSON body");
                        if let Some(message) = json.get("message").and_then(|m| m.as_str()) {
                            log(&format!("Adding message: {}", message));
                            // Add the message to our state
                            app_state.messages.push(message.to_string());

                            // Return success
                            let data = serde_json::json!({
                                "success": true,
                                "message": "Message added successfully"
                            });
                            let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
                            log("Message added successfully");

                            HttpResponse {
                                status: 200,
                                headers: vec![(
                                    "content-type".to_string(),
                                    "application/json".to_string(),
                                )],
                                body: Some(body),
                            }
                        } else {
                            // No message field found
                            log("Error: No message field found in request");
                            let data = serde_json::json!({
                                "success": false,
                                "error": "No message field found in request"
                            });
                            let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;

                            HttpResponse {
                                status: 400,
                                headers: vec![(
                                    "content-type".to_string(),
                                    "application/json".to_string(),
                                )],
                                body: Some(body),
                            }
                        }
                    } else {
                        // Invalid JSON
                        log("Error: Invalid JSON in request body");
                        let data = serde_json::json!({
                            "success": false,
                            "error": "Invalid JSON in request body"
                        });
                        let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;

                        HttpResponse {
                            status: 400,
                            headers: vec![(
                                "content-type".to_string(),
                                "application/json".to_string(),
                            )],
                            body: Some(body),
                        }
                    }
                } else {
                    // No body provided
                    log("Error: No request body provided");
                    let data = serde_json::json!({
                        "success": false,
                        "error": "No request body provided"
                    });
                    let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;

                    HttpResponse {
                        status: 400,
                        headers: vec![("content-type".to_string(), "application/json".to_string())],
                        body: Some(body),
                    }
                }
            }
            _ => {
                // Path not found
                log(&format!(
                    "Error: Path not found - {} {}",
                    request.method, request.uri
                ));
                let data = serde_json::json!({
                    "success": false,
                    "error": "Not found"
                });
                let body = serde_json::to_vec(&data).map_err(|e| e.to_string())?;

                HttpResponse {
                    status: 404,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: Some(body),
                }
            }
        };

        // Save the updated state
        log(&format!(
            "Saving updated state: count={}, messages={}",
            app_state.count,
            app_state.messages.len()
        ));
        let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
        let updated_state = Some(updated_state_bytes);

        Ok((updated_state, (response,)))
    }

    // Middleware Handler
    fn handle_middleware(
        state: State,
        params: (u64, HttpRequest),
    ) -> Result<(State, (MiddlewareResult,)), String> {
        let (handler_id, request) = params;
        log(&format!(
            "Handling middleware with handler ID: {}",
            handler_id
        ));

        // Check for an API key header
        log("Checking for API key header");
        let auth_header = request
            .headers
            .iter()
            .find(|(name, _)| name.to_lowercase() == "x-api-key");

        if let Some((_, value)) = auth_header {
            log(&format!("Found API key header: {}", value));
            // Check if the API key is valid
            if value == "theater-demo-key" {
                // Allow the request to proceed
                log("API key is valid, allowing request to proceed");
                Ok((
                    state,
                    (MiddlewareResult {
                        proceed: true,
                        request,
                    },),
                ))
            } else {
                // Invalid API key
                log(&format!("Invalid API key: {}", value));
                Ok((
                    state,
                    (MiddlewareResult {
                        proceed: false,
                        request,
                    },),
                ))
            }
        } else {
            // No API key provided
            log("No API key provided");
            Ok((
                state,
                (MiddlewareResult {
                    proceed: false,
                    request,
                },),
            ))
        }
    }

    // WebSocket Connect Handler
    fn handle_websocket_connect(
        state: State,
        params: (u64, u64, String, Option<String>),
    ) -> Result<(State,), String> {
        let (handler_id, connection_id, path, query) = params;
        log(&format!(
            "WebSocket connected - Handler: {}, Connection: {}, Path: {}",
            handler_id, connection_id, path
        ));

        if let Some(q) = query {
            log(&format!("  Query parameters: {}", q));
        }

        Ok((state,))
    }

    // WebSocket Message Handler
    fn handle_websocket_message(
        state: State,
        params: (u64, u64, WebsocketMessage),
    ) -> Result<(State, (Vec<WebsocketMessage>,)), String> {
        let (handler_id, connection_id, message) = params;
        log(&format!(
            "WebSocket message received - Handler: {}, Connection: {}",
            handler_id, connection_id
        ));

        // Parse the current state
        let state_bytes = state.unwrap_or_default();
        let mut app_state: AppState = if !state_bytes.is_empty() {
            log("WebSocket: Deserializing existing state");
            let app_state: AppState =
                serde_json::from_slice(&state_bytes).map_err(|e| e.to_string())?;
            app_state
        } else {
            log("WebSocket: Creating default state");
            AppState::default()
        };

        let responses = match message.ty {
            MessageType::Text => {
                // Echo the message back
                if let Some(text) = message.text {
                    log(&format!("  Text message: {}", text));

                    // Add the message to our state
                    app_state.messages.push(text.clone());

                    // Echo back the message
                    let echo_message = WebsocketMessage {
                        ty: MessageType::Text,
                        data: None,
                        text: Some(format!("Echo: {}", text)),
                    };

                    // Also send the current count
                    let count_message = WebsocketMessage {
                        ty: MessageType::Text,
                        data: None,
                        text: Some(format!("Current count: {}", app_state.count)),
                    };

                    vec![echo_message, count_message]
                } else {
                    vec![]
                }
            }
            MessageType::Binary => {
                // Echo binary data
                if let Some(data) = message.data {
                    log(&format!("  Binary message: {} bytes", data.len()));

                    // Just echo it back
                    vec![WebsocketMessage {
                        ty: MessageType::Binary,
                        data: Some(data),
                        text: None,
                    }]
                } else {
                    vec![]
                }
            }
            MessageType::Ping => {
                // Respond to ping with pong
                log("  Ping received");
                vec![WebsocketMessage {
                    ty: MessageType::Pong,
                    data: None,
                    text: None,
                }]
            }
            _ => {
                // Other message types
                log("  Other message type");
                vec![]
            }
        };

        // Save the updated state
        let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
        let updated_state = Some(updated_state_bytes);

        Ok((updated_state, (responses,)))
    }

    // WebSocket Disconnect Handler
    fn handle_websocket_disconnect(state: State, params: (u64, u64)) -> Result<(State,), String> {
        let (handler_id, connection_id) = params;
        log(&format!(
            "WebSocket disconnected - Handler: {}, Connection: {}",
            handler_id, connection_id
        ));

        Ok((state,))
    }
}

impl MessageServerClient for Actor {
    fn handle_send(
        state: Option<Vec<u8>>,
        _params: (Vec<u8>,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        Ok((state,))
    }

    fn handle_request(
        state: Option<Vec<u8>>,
        _params: (Vec<u8>,),
    ) -> Result<(Option<Vec<u8>>, (Vec<u8>,)), String> {
        Ok((state, (vec![],)))
    }

    fn handle_channel_open(
        state: Option<bindings::exports::ntwk::theater::message_server_client::Json>,
        params: (bindings::exports::ntwk::theater::message_server_client::Json,),
    ) -> Result<
        (
            Option<bindings::exports::ntwk::theater::message_server_client::Json>,
            (bindings::exports::ntwk::theater::message_server_client::ChannelAccept,),
        ),
        String,
    > {
        Ok((
            state,
            (
                bindings::exports::ntwk::theater::message_server_client::ChannelAccept {
                    accepted: true,
                    message: None,
                },
            ),
        ))
    }

    fn handle_channel_close(
        state: Option<bindings::exports::ntwk::theater::message_server_client::Json>,
        params: (String,),
    ) -> Result<(Option<bindings::exports::ntwk::theater::message_server_client::Json>,), String>
    {
        Ok((state,))
    }

    fn handle_channel_message(
        state: Option<bindings::exports::ntwk::theater::message_server_client::Json>,
        params: (
            String,
            bindings::exports::ntwk::theater::message_server_client::Json,
        ),
    ) -> Result<(Option<bindings::exports::ntwk::theater::message_server_client::Json>,), String>
    {
        log("runtime-content-fs: Received channel message");
        Ok((state,))
    }
}

bindings::export!(Actor with_types_in bindings);
"#);

    // Add README.md
    files.insert("README.md", r#"# {{project_name}}

An HTTP server Theater actor created from the template.

## Building

To build the actor:

```bash
cargo build --target wasm32-unknown-unknown --release
```

## Running

To run the actor with Theater:

```bash
theater start manifest.toml
```

## Features

This HTTP actor provides:

- RESTful API endpoints
- WebSocket support
- Middleware for authentication
- State management
- Message handling

## API Endpoints

The actor exposes the following HTTP endpoints:

- `GET /api/count` - Get the current count
- `POST /api/count` - Increment the count
- `GET /api/messages` - Get all stored messages
- `POST /api/messages` - Add a new message

## WebSocket

The actor also supports WebSocket connections at `/ws`. The WebSocket interface:

- Echoes back text messages
- Returns the current count
- Stores new messages in the actor state

## Authentication

API endpoints under `/api/*` are protected with a simple API key middleware.
Include the header `X-API-Key: theater-demo-key` in your requests.

## Example Usage

```bash
# Get the current count
curl -H "X-API-Key: theater-demo-key" http://localhost:8080/api/count

# Add a new message
curl -X POST -H "Content-Type: application/json" \
     -H "X-API-Key: theater-demo-key" \
     -d '{"message":"Hello, Theater!"}' \
     http://localhost:8080/api/messages
```

For WebSocket testing, you can use a tool like websocat:

```bash
websocat ws://localhost:8080/ws
```
"#);

    files
}

/// Create a new actor project from a template
pub fn create_project(
    template_name: &str,
    project_name: &str,
    target_dir: &Path,
) -> Result<(), io::Error> {
    let templates = available_templates();
    let template = templates
        .get(template_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Template not found"))?;

    info!(
        "Creating new {} project '{}' in {}",
        template_name,
        project_name,
        target_dir.display()
    );

    // Create the target directory
    fs::create_dir_all(target_dir)?;

    // Create all template files
    for (relative_path, content) in &template.files {
        let file_path = target_dir.join(relative_path);

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Replace template variables
        let processed_content = content.replace("{{project_name}}", project_name);
        
        // Also handle snake_case version for file paths
        let project_name_snake = project_name.replace('-', "_");
        let processed_content = processed_content.replace("{{project_name_snake}}", &project_name_snake);

        debug!(
            "Creating file: {} ({} bytes)",
            file_path.display(),
            processed_content.len()
        );

        // Write the file
        fs::write(&file_path, processed_content)?;
    }

    info!("Project '{}' created successfully!", project_name);
    Ok(())
}

/// List all available templates
pub fn list_templates() {
    let templates = available_templates();
    
    println!("Available templates:");
    for (name, template) in templates {
        println!("  {}: {}", name, template.description);
    }
}
