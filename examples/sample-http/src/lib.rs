mod bindings;

use crate::bindings::exports::ntwk::theater::actor::Guest;
use crate::bindings::exports::ntwk::theater::http_handlers::Guest as HttpHandlers;
use crate::bindings::exports::ntwk::theater::message_server_client::Guest as MessageServerClient;
use crate::bindings::ntwk::theater::http_framework::{
    add_middleware, add_route, create_server, enable_websocket, get_server_info, register_handler,
    start_server, ServerConfig,
};
use crate::bindings::ntwk::theater::http_types::{HttpRequest, HttpResponse, MiddlewareResult};
use crate::bindings::ntwk::theater::runtime::log;
use crate::bindings::ntwk::theater::types::State;
use crate::bindings::ntwk::theater::websocket_types::{MessageType, WebsocketMessage};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    fn init(state: State, params: (String,)) -> Result<(State,), String> {
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

    // Get server info
    let info = get_server_info(server_id)?;
    log(&format!(
        "Server info: Port: {}, Running: {}",
        info.port, info.running
    ));

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

        // In a real application, this might check for authentication tokens
        // For this example, we'll check for an API key header
        log("Checking for API key header");
        let auth_header = request
            .headers
            .iter()
            .find(|(name, _)| name.to_lowercase() == "x-api-key");

        log(&format!("Request has {} headers", request.headers.len()));
        for (idx, (name, value)) in request.headers.iter().enumerate() {
            log(&format!("Header {}: '{}' = '{}'", idx, name, value));
        }

        if let Some((name, value)) = auth_header {
            log(&format!("Found API key header: '{}' = '{}'", name, value));
            // Check if the API key is valid (very simple check)
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
            log(&format!(
                "WebSocket: Current state: count={}, messages={}",
                app_state.count,
                app_state.messages.len()
            ));
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
        log(&format!(
            "WebSocket: Saving updated state: count={}, messages={}",
            app_state.count,
            app_state.messages.len()
        ));
        let updated_state_bytes = serde_json::to_vec(&app_state).map_err(|e| e.to_string())?;
        let updated_state = Some(updated_state_bytes);

        log(&format!(
            "WebSocket: Sending {} response messages",
            responses.len()
        ));
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
}

bindings::export!(Actor with_types_in bindings);
