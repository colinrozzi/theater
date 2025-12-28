mod bindings;

use bindings::{
    exports::theater::simple::http_handlers::Guest as HttpHandlers,
    exports::theater::simple::actor::Guest as Actor,
    theater::simple::{
        http_client,
        http_framework::{self, HandlerId},
        http_types::{HttpRequest, HttpResponse, MiddlewareResult, ServerConfig},
        websocket_types::WebsocketMessage,
    },
};

struct Component;

// Configuration
const UPSTREAM_BASE_URL: &str = "http://httpbin.org";
const PROXY_PORT: u16 = 8080;

impl Actor for Component {
    fn init(
        _state: Option<Vec<u8>>,
        _params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Create HTTP server
        let server_id = http_framework::create_server(&ServerConfig {
            port: Some(PROXY_PORT),
            host: Some("127.0.0.1".to_string()),
            tls_config: None,
        })?;

        // Register handler for proxy
        let handler_id = http_framework::register_handler("proxy")?;

        // Add catch-all route for all methods and paths
        // Route all HTTP methods through our proxy handler
        for method in &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"] {
            http_framework::add_route(
                server_id,
                "/{*path}", // Catch-all wildcard pattern
                method,
                handler_id,
            )?;
        }

        // Start the server
        let _port = http_framework::start_server(server_id)?;

        // Return success - state is stored by Theater framework
        Ok((Some(format!("Proxy started, forwarding to {}", UPSTREAM_BASE_URL).into_bytes()),))
    }
}

impl HttpHandlers for Component {
    fn handle_request(
        state: Option<Vec<u8>>,
        params: (HandlerId, HttpRequest),
    ) -> Result<(Option<Vec<u8>>, (HttpResponse,)), String> {
        let (_handler_id, request) = params;

        // Build upstream URL by combining base URL with request path
        let upstream_url = if request.uri.starts_with('/') {
            format!("{}{}", UPSTREAM_BASE_URL, request.uri)
        } else {
            format!("{}/{}", UPSTREAM_BASE_URL, request.uri)
        };

        // Create upstream request with modified URI
        let upstream_request = HttpRequest {
            method: request.method.clone(),
            uri: upstream_url,
            headers: request.headers.clone(),
            body: request.body.clone(),
        };

        // Forward request to upstream server
        match http_client::send_http(&upstream_request) {
            Ok(response) => {
                // Return the upstream response
                Ok((state, (response,)))
            }
            Err(e) => {
                // If upstream fails, return 502 Bad Gateway
                let error_response = HttpResponse {
                    status: 502,
                    headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
                    body: Some(format!("Upstream error: {}", e).into_bytes()),
                };
                Ok((state, (error_response,)))
            }
        }
    }

    fn handle_middleware(
        state: Option<Vec<u8>>,
        params: (HandlerId, HttpRequest),
    ) -> Result<(Option<Vec<u8>>, (MiddlewareResult,)), String> {
        let (_handler_id, request) = params;

        // Pass through all requests - no middleware logic needed for basic proxy
        Ok((state, (MiddlewareResult {
            proceed: true,
            request,
        },)))
    }

    fn handle_websocket_connect(
        state: Option<Vec<u8>>,
        _params: (HandlerId, u64, String, Option<String>),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Not implementing WebSocket proxying in this basic example
        Ok((state,))
    }

    fn handle_websocket_message(
        state: Option<Vec<u8>>,
        _params: (HandlerId, u64, WebsocketMessage),
    ) -> Result<(Option<Vec<u8>>, (Vec<WebsocketMessage>,)), String> {
        // Not implementing WebSocket proxying in this basic example
        Ok((state, (vec![],)))
    }

    fn handle_websocket_disconnect(
        state: Option<Vec<u8>>,
        _params: (HandlerId, u64),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Not implementing WebSocket proxying in this basic example
        Ok((state,))
    }
}

bindings::export!(Component with_types_in bindings);
