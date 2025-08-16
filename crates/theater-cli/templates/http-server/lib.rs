#[allow(warnings)]
mod bindings;

use bindings::exports::theater::simple::actor::Guest;
use bindings::exports::theater::simple::http_handlers::Guest as HttpHandlersGuest;
use bindings::theater::simple::http_framework::{self, HandlerId, ServerId};
use bindings::theater::simple::http_types::{HttpRequest, HttpResponse, ServerConfig};
use bindings::theater::simple::runtime;

struct Component;

// Simple state to track our HTTP server
#[derive(Clone)]
struct ServerState {
    server_id: ServerId,
}

// --- State Management ---

fn get_server_state(state_bytes: &Option<Vec<u8>>) -> Result<ServerState, String> {
    let bytes = state_bytes.as_ref().ok_or("Server state is not set")?;
    let state_str = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
    let server_id = state_str.parse::<u64>().map_err(|e| e.to_string())?;
    
    Ok(ServerState { server_id })
}

fn set_server_state(state: &ServerState) -> Vec<u8> {
    state.server_id.to_string().into_bytes()
}

// --- Actor Implementation ---

impl Guest for Component {
    fn init(_data: Option<Vec<u8>>, params: (String,)) -> Result<(Option<Vec<u8>>,), String> {
        let (actor_id,) = params;
        runtime::log(&format!("{{project_name}} HTTP server actor {} initializing", actor_id));

        // Create HTTP server configuration
        let config = ServerConfig {
            port: Some(8080),
            host: Some("0.0.0.0".to_string()),
            tls_config: None,
        };

        // Create the HTTP server
        let server_id = http_framework::create_server(&config).map_err(|e| e.to_string())?;

        // Register handler for our routes
        let main_handler = http_framework::register_handler("main").map_err(|e| e.to_string())?;

        // Set up routes
        http_framework::add_route(server_id, "/", "GET", main_handler)
            .map_err(|e| e.to_string())?;
        http_framework::add_route(server_id, "/health", "GET", main_handler)
            .map_err(|e| e.to_string())?;

        // Start the server
        http_framework::start_server(server_id).map_err(|e| e.to_string())?;
        
        runtime::log("{{project_name}} HTTP server started on port 8080");
        runtime::log("Available endpoints:");
        runtime::log("  GET / - Welcome message");
        runtime::log("  GET /health - Health check");

        // Save server state
        let initial_state = ServerState { server_id };
        Ok((Some(set_server_state(&initial_state)),))
    }
}

impl HttpHandlersGuest for Component {
    fn handle_request(
        state_bytes: Option<Vec<u8>>,
        params: (HandlerId, HttpRequest),
    ) -> Result<(Option<Vec<u8>>, (HttpResponse,)), String> {
        let (_handler_id, request) = params;
        
        runtime::log(&format!(
            "Handling {} request to {}",
            request.method, request.uri
        ));

        let response = match (request.method.as_str(), request.uri.as_str()) {
            ("GET", "/") => generate_welcome_response(),
            ("GET", "/health") => generate_health_response(),
            _ => generate_404_response(),
        };

        Ok((state_bytes, (response,)))
    }

    fn handle_middleware(
        state: Option<Vec<u8>>,
        params: (HandlerId, HttpRequest),
    ) -> Result<
        (
            Option<Vec<u8>>,
            (bindings::theater::simple::http_types::MiddlewareResult,),
        ),
        String,
    > {
        let (_, request) = params;
        
        // Simple middleware - just log and proceed
        runtime::log(&format!("{{project_name}} request: {} {}", request.method, request.uri));
        
        let middleware_result = bindings::theater::simple::http_types::MiddlewareResult {
            proceed: true,
            request,
        };
        
        Ok((state, (middleware_result,)))
    }

    fn handle_websocket_connect(
        state: Option<Vec<u8>>,
        _: (HandlerId, u64, String, Option<String>),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // WebSocket not implemented yet - just return state unchanged
        Ok((state,))
    }

    fn handle_websocket_message(
        state: Option<Vec<u8>>,
        _: (
            HandlerId,
            u64,
            bindings::theater::simple::websocket_types::WebsocketMessage,
        ),
    ) -> Result<
        (
            Option<Vec<u8>>,
            (Vec<bindings::theater::simple::websocket_types::WebsocketMessage>,),
        ),
        String,
    > {
        // WebSocket not implemented yet - return no messages
        Ok((state, (vec![],)))
    }

    fn handle_websocket_disconnect(
        state: Option<Vec<u8>>,
        _: (HandlerId, u64),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // WebSocket not implemented yet - just return state unchanged
        Ok((state,))
    }
}

// --- Response Generation Functions ---

fn generate_welcome_response() -> HttpResponse {
    let html_content = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{{project_name}} HTTP Server</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            line-height: 1.6;
            max-width: 800px;
            margin: 0 auto;
            padding: 2rem;
            background: #f8fafc;
        }
        .container {
            background: white;
            padding: 2rem;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }
        h1 {
            color: #2563eb;
            margin-bottom: 1rem;
        }
        .endpoints {
            background: #f1f5f9;
            padding: 1rem;
            border-radius: 6px;
            margin-top: 1.5rem;
        }
        code {
            background: #e2e8f0;
            padding: 0.2rem 0.4rem;
            border-radius: 3px;
            font-family: 'Monaco', 'Consolas', monospace;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Hello from {{project_name}}! 🎭</h1>
        <p>Your Theater WebAssembly HTTP server is running successfully!</p>
        
        <div class="endpoints">
            <h3>Available Endpoints:</h3>
            <ul>
                <li><code>GET /</code> - This welcome page</li>
                <li><code>GET /health</code> - Health check (JSON)</li>
            </ul>
        </div>
        
        <p style="margin-top: 1.5rem; color: #64748b;">
            Built with Theater WebAssembly actors • 
            <a href="https://github.com/colinrozzi/theater" style="color: #2563eb;">Learn more</a>
        </p>
    </div>
</body>
</html>"#;

    HttpResponse {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/html; charset=utf-8".to_string(),
        )],
        body: Some(html_content.as_bytes().to_vec()),
    }
}

fn generate_health_response() -> HttpResponse {
    let json_body = r#"{"status":"ok","service":"{{project_name}}","message":"HTTP server is running"}"#;
    
    HttpResponse {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )],
        body: Some(json_body.as_bytes().to_vec()),
    }
}

fn generate_404_response() -> HttpResponse {
    let html_content = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>404 - Not Found</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            text-align: center;
            padding: 2rem;
            background: #f8fafc;
        }
        .container {
            max-width: 500px;
            margin: 0 auto;
            background: white;
            padding: 2rem;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }
        h1 { color: #dc2626; }
    </style>
</head>
<body>
    <div class="container">
        <h1>404 - Not Found</h1>
        <p>The requested page could not be found.</p>
        <a href="/">← Back to Home</a>
    </div>
</body>
</html>"#;

    HttpResponse {
        status: 404,
        headers: vec![(
            "Content-Type".to_string(),
            "text/html; charset=utf-8".to_string(),
        )],
        body: Some(html_content.as_bytes().to_vec()),
    }
}

bindings::export!(Component with_types_in bindings);
