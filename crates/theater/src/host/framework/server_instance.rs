use crate::actor::handle::ActorHandle;
use axum::{
    extract::{MatchedPath, Path, State, WebSocketUpgrade},
    http::{HeaderName, HeaderValue, Request, StatusCode},
    response::Response,
    routing::{any, delete, get, head, options, patch, post, put},
    Router,
};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::panic;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::tls::{create_tls_config, validate_tls_config};
use super::types::*;

// Route configuration
#[derive(Clone)]
#[allow(dead_code)]
pub struct RouteConfig {
    pub id: u64,
    pub path: String,
    pub method: String,
    pub handler_id: u64,
}

// Middleware configuration
#[derive(Clone)]
#[allow(dead_code)]
pub struct MiddlewareConfig {
    pub id: u64,
    pub path: String,
    pub handler_id: u64,
    pub priority: u32, // Lower numbers run first
}

// WebSocket configuration
#[derive(Clone)]
#[allow(dead_code)]
pub struct WebSocketConfig {
    pub path: String,
    pub connect_handler_id: Option<u64>,
    pub message_handler_id: u64,
    pub disconnect_handler_id: Option<u64>,
}

// WebSocket connection
pub struct WebSocketConnection {
    pub id: u64,
    pub sender: mpsc::Sender<WebSocketMessage>,
}

// New: State for individual route handlers
#[derive(Clone)]
pub struct RouteHandlerState {
    pub handler_id: u64,
    pub server_state: Arc<ServerState>,
}

// New: State for WebSocket handlers
#[derive(Clone)]
pub struct WebSocketHandlerState {
    pub config: WebSocketConfig,
    pub server_state: Arc<ServerState>,
}

// Simplified server state (no more route mapping needed)
pub struct ServerState {
    pub id: u64,
    pub actor_handle: ActorHandle,
    pub middlewares: HashMap<u64, MiddlewareConfig>,
    pub websockets: HashMap<String, WebSocketConfig>,
    pub active_ws_connections: Arc<RwLock<HashMap<u64, WebSocketConnection>>>,
}

pub struct ServerInstance {
    id: u64,
    config: ServerConfig,
    routes: HashMap<u64, RouteConfig>,
    middlewares: HashMap<u64, MiddlewareConfig>,
    websockets: HashMap<String, WebSocketConfig>,
    pub active_ws_connections: Arc<RwLock<HashMap<u64, WebSocketConnection>>>,
    server_handle: Option<JoinHandle<()>>,
    port: u16,
    running: bool,
    // Channel to signal server shutdown
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ServerInstance {
    pub fn new(id: u64, port: u16, host: String, tls_config: Option<TlsConfig>) -> Self {
        Self {
            id,
            config: ServerConfig {
                port: Some(port),
                host: Some(host),
                tls_config,
            },
            routes: HashMap::new(),
            middlewares: HashMap::new(),
            websockets: HashMap::new(),
            active_ws_connections: Arc::new(RwLock::new(HashMap::new())),
            server_handle: None,
            port,
            running: false,
            shutdown_tx: None,
        }
    }

    pub fn get_info(&self) -> ServerInfo {
        ServerInfo {
            id: self.id,
            port: self.port,
            host: self
                .config
                .host
                .clone()
                .unwrap_or_else(|| "0.0.0.0".to_string()),
            running: self.running,
            routes_count: self.routes.len() as u32,
            middleware_count: self.middlewares.len() as u32,
            websocket_enabled: !self.websockets.is_empty(),
        }
    }

    pub fn is_https(&self) -> bool {
        self.config.tls_config.is_some()
    }

    pub fn get_tls_cert_path(&self) -> Option<&str> {
        self.config
            .tls_config
            .as_ref()
            .map(|tls| tls.cert_path.as_str())
    }

    // Existing route/middleware/websocket management methods remain the same
    pub async fn add_route(
        &mut self,
        route_id: u64,
        path: String,
        method: String,
        handler_id: u64,
    ) -> Result<()> {
        // Validate path and method
        if !path.starts_with('/') {
            return Err(anyhow::anyhow!("Path must start with /"));
        }

        // Validate route pattern before adding
        if let Err(e) = validate_route_pattern(&path) {
            return Err(anyhow::anyhow!("Invalid route pattern: {}", e));
        }

        let route_config = RouteConfig {
            id: route_id,
            path,
            method,
            handler_id,
        };

        self.routes.insert(route_id, route_config);
        Ok(())
    }

    pub fn remove_route(&mut self, route_id: u64) -> Result<()> {
        if self.routes.remove(&route_id).is_some() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Route not found: {}", route_id))
        }
    }

    pub fn has_route(&self, route_id: u64) -> bool {
        self.routes.contains_key(&route_id)
    }

    pub fn add_middleware(
        &mut self,
        middleware_id: u64,
        path: String,
        handler_id: u64,
    ) -> Result<()> {
        let middleware_config = MiddlewareConfig {
            id: middleware_id,
            path,
            handler_id,
            priority: 100, // Default priority
        };

        self.middlewares.insert(middleware_id, middleware_config);
        Ok(())
    }

    pub fn remove_middleware(&mut self, middleware_id: u64) -> Result<()> {
        if self.middlewares.remove(&middleware_id).is_some() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Middleware not found: {}", middleware_id))
        }
    }

    pub fn has_middleware(&self, middleware_id: u64) -> bool {
        self.middlewares.contains_key(&middleware_id)
    }

    pub fn enable_websocket(
        &mut self,
        path: String,
        connect_handler_id: Option<u64>,
        message_handler_id: u64,
        disconnect_handler_id: Option<u64>,
    ) -> Result<()> {
        let ws_config = WebSocketConfig {
            path: path.clone(),
            connect_handler_id,
            message_handler_id,
            disconnect_handler_id,
        };

        self.websockets.insert(path, ws_config);
        Ok(())
    }

    pub fn disable_websocket(&mut self, path: &str) -> Result<()> {
        if self.websockets.remove(path).is_some() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("WebSocket not found for path: {}", path))
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    // NEW: Completely rewritten router building using Axum's native routing
    fn build_router(&self, actor_handle: ActorHandle) -> Router {
        let mut router = Router::new();

        // Create shared base state
        let base_state = Arc::new(ServerState {
            id: self.id,
            actor_handle,
            middlewares: self.middlewares.clone(),
            websockets: self.websockets.clone(),
            active_ws_connections: self.active_ws_connections.clone(),
        });

        // Add each route individually with Axum's native method routing
        for route in self.routes.values() {
            let route_state = RouteHandlerState {
                handler_id: route.handler_id,
                server_state: base_state.clone(),
            };

            debug!(
                "Adding route: {} {} -> handler {}",
                route.method, route.path, route.handler_id
            );

            // Use the appropriate HTTP method handler - Axum handles all the wildcard magic!
            let method_handler = match route.method.as_str() {
                "GET" => get(Self::handle_request),
                "POST" => post(Self::handle_request),
                "PUT" => put(Self::handle_request),
                "DELETE" => delete(Self::handle_request),
                "PATCH" => patch(Self::handle_request),
                "HEAD" => head(Self::handle_request),
                "OPTIONS" => options(Self::handle_request),
                "*" => any(Self::handle_request),
                _ => {
                    warn!("Unknown HTTP method: {}, using any()", route.method);
                    any(Self::handle_request)
                }
            };

            // Let Axum handle the path pattern matching - this is where the magic happens!
            router = router.route(&route.path, method_handler.with_state(route_state));
        }

        // Add WebSocket routes
        for (ws_path, ws_config) in &self.websockets {
            let ws_state = WebSocketHandlerState {
                config: ws_config.clone(),
                server_state: base_state.clone(),
            };

            debug!("Adding WebSocket route: {}", ws_path);
            router = router.route(
                ws_path,
                get(Self::handle_websocket_upgrade).with_state(ws_state),
            );
        }

        info!(
            "Built router with {} routes and {} WebSocket endpoints",
            self.routes.len(),
            self.websockets.len()
        );
        router
    }

    // NEW: Much simpler request handler - Axum has done all the routing work!
    async fn handle_request(
        State(route_state): State<RouteHandlerState>,
        matched_path: MatchedPath,
        // Axum automatically extracts path parameters based on the route pattern
        path_params: Option<Path<HashMap<String, String>>>,
        req: Request<axum::body::Body>,
    ) -> Response<axum::body::Body> {
        let actual_path = req.uri().path().to_string();
        let method = req.method().as_str().to_uppercase();

        debug!(
            "Handling {} request to {} (matched pattern: {})",
            method,
            actual_path,
            matched_path.as_str()
        );

        // Convert request, including path parameters extracted by Axum
        let mut theater_request = convert_request_with_axum_params(req, path_params).await;

        // Apply middlewares (using the actual requested path, not the pattern)
        let middlewares = route_state.server_state.find_middlewares(&actual_path);
        for middleware in middlewares {
            match route_state
                .server_state
                .call_middleware(middleware.handler_id, theater_request.clone())
                .await
            {
                Ok(result) => {
                    if !result.proceed {
                        debug!("Middleware {} rejected request", middleware.handler_id);
                        return Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .body(axum::body::Body::empty())
                            .unwrap_or_default();
                    }
                    theater_request = result.request;
                }
                Err(e) => {
                    error!("Middleware error: {}", e);
                    return Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(format!("Middleware error: {}", e).into())
                        .unwrap_or_default();
                }
            }
        }

        // Call the specific handler for this route
        match route_state
            .server_state
            .call_route_handler(route_state.handler_id, theater_request)
            .await
        {
            Ok(response) => {
                debug!("Handler {} completed successfully", route_state.handler_id);
                convert_response(response)
            }
            Err(e) => {
                error!(
                    "Handler error for route {} ({}): {}",
                    matched_path.as_str(),
                    route_state.handler_id,
                    e
                );
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(format!("Handler error: {}", e).into())
                    .unwrap_or_default()
            }
        }
    }

    // Updated WebSocket handler to use new state structure
    async fn handle_websocket_upgrade(
        State(ws_state): State<WebSocketHandlerState>,
        ws: WebSocketUpgrade,
        req: Request<axum::body::Body>,
    ) -> Response<axum::body::Body> {
        let path = req.uri().path().to_string();

        ws.on_upgrade(move |socket| async move {
            Self::handle_websocket_connection(ws_state, socket, path).await;
        })
    }

    async fn handle_websocket_connection(
        ws_state: WebSocketHandlerState,
        socket: axum::extract::ws::WebSocket,
        path: String,
    ) {
        let connection_id = rand::random::<u64>();
        let (mut sender, mut receiver) = socket.split();
        let (tx, mut rx) = mpsc::channel::<WebSocketMessage>(100);

        // Store connection
        {
            let mut connections = ws_state.server_state.active_ws_connections.write().await;
            connections.insert(
                connection_id,
                WebSocketConnection {
                    id: connection_id,
                    sender: tx,
                },
            );
        }

        // Handle connect event
        if let Some(connect_handler_id) = ws_state.config.connect_handler_id {
            if let Err(e) = ws_state
                .server_state
                .actor_handle
                .call_function::<(u64, u64, String, Option<String>), ()>(
                    "theater:simple/http-handlers.handle-websocket-connect".to_string(),
                    (connect_handler_id, connection_id, path.clone(), None),
                )
                .await
            {
                error!("WebSocket connect handler error: {}", e);
            }
        }

        // Spawn task to send messages from channel to WebSocket
        let send_task = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let ws_message = match message.ty {
                    MessageType::Text => {
                        if let Some(text) = message.text {
                            axum::extract::ws::Message::Text(text.into())
                        } else {
                            continue;
                        }
                    }
                    MessageType::Binary => {
                        if let Some(data) = message.data {
                            axum::extract::ws::Message::Binary(data.into())
                        } else {
                            continue;
                        }
                    }
                    MessageType::Close => axum::extract::ws::Message::Close(None),
                    MessageType::Ping => {
                        if let Some(data) = message.data {
                            axum::extract::ws::Message::Ping(data.into())
                        } else {
                            axum::extract::ws::Message::Ping(vec![].into())
                        }
                    }
                    MessageType::Pong => {
                        if let Some(data) = message.data {
                            axum::extract::ws::Message::Pong(data.into())
                        } else {
                            axum::extract::ws::Message::Pong(vec![].into())
                        }
                    }
                    _ => continue,
                };

                if sender.send(ws_message).await.is_err() {
                    break;
                }
            }
        });

        // Handle incoming messages
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    let message = WebSocketMessage {
                        ty: MessageType::Text,
                        data: None,
                        text: Some(text.to_string()),
                    };

                    if let Err(e) = ws_state
                        .server_state
                        .actor_handle
                        .call_function::<(u64, u64, WebSocketMessage), (Vec<WebSocketMessage>,)>(
                            "theater:simple/http-handlers.handle-websocket-message".to_string(),
                            (ws_state.config.message_handler_id, connection_id, message),
                        )
                        .await
                    {
                        error!("WebSocket message handler error: {}", e);
                    }
                }
                Ok(axum::extract::ws::Message::Binary(data)) => {
                    let message = WebSocketMessage {
                        ty: MessageType::Binary,
                        data: Some(data.to_vec()),
                        text: None,
                    };

                    if let Err(e) = ws_state
                        .server_state
                        .actor_handle
                        .call_function::<(u64, u64, WebSocketMessage), (Vec<WebSocketMessage>,)>(
                            "theater:simple/http-handlers.handle-websocket-message".to_string(),
                            (ws_state.config.message_handler_id, connection_id, message),
                        )
                        .await
                    {
                        error!("WebSocket message handler error: {}", e);
                    }
                }
                Ok(axum::extract::ws::Message::Close(_)) => {
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        // Clean up connection
        {
            let mut connections = ws_state.server_state.active_ws_connections.write().await;
            connections.remove(&connection_id);
        }

        // Handle disconnect event
        if let Some(disconnect_handler_id) = ws_state.config.disconnect_handler_id {
            if let Err(e) = ws_state
                .server_state
                .actor_handle
                .call_function::<(u64, u64), ()>(
                    "theater:simple/http-handlers.handle-websocket-disconnect".to_string(),
                    (disconnect_handler_id, connection_id),
                )
                .await
            {
                error!("WebSocket disconnect handler error: {}", e);
            }
        }

        send_task.abort();
    }

    pub async fn start(&mut self, actor_handle: ActorHandle) -> Result<u16> {
        if self.running {
            return Err(anyhow::anyhow!("Server is already running"));
        }

        // Check if TLS is configured
        if let Some(tls_config) = self.config.tls_config.clone() {
            self.start_https(actor_handle, &tls_config).await
        } else {
            self.start_http(actor_handle).await
        }
    }

    async fn start_http(&mut self, actor_handle: ActorHandle) -> Result<u16> {
        let router = self
            .build_router_safe(actor_handle)
            .map_err(|e| anyhow::anyhow!(e))?;
        let host = self
            .config
            .host
            .clone()
            .unwrap_or_else(|| "0.0.0.0".to_string());

        let addr = if self.port == 0 {
            format!("{}:0", host)
        } else {
            format!("{}:{}", host, self.port)
        };

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let actual_addr = listener.local_addr()?;
        self.port = actual_addr.port();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let server_handle = tokio::spawn(async move {
            let graceful = axum::serve(listener, router).with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            });

            if let Err(e) = graceful.await {
                error!("Server error: {}", e);
            }
        });

        self.server_handle = Some(server_handle);
        self.running = true;

        info!("HTTP server {} started on {}:{}", self.id, host, self.port);
        Ok(self.port)
    }

    async fn start_https(
        &mut self,
        actor_handle: ActorHandle,
        tls_config: &TlsConfig,
    ) -> Result<u16> {
        // Validate TLS configuration early
        debug!("Validating TLS configuration for server {}", self.id);
        validate_tls_config(&tls_config.cert_path, &tls_config.key_path)
            .map_err(|e| anyhow::anyhow!("TLS validation failed: {}", e))?;

        // Build router
        let router = self
            .build_router_safe(actor_handle)
            .map_err(|e| anyhow::anyhow!(e))?;
        let host = self
            .config
            .host
            .clone()
            .unwrap_or_else(|| "0.0.0.0".to_string());

        let addr = if self.port == 0 {
            format!("{}:0", host)
        } else {
            format!("{}:{}", host, self.port)
        };

        // Create TLS configuration
        debug!("Loading TLS certificates for server {}", self.id);
        let rustls_config = create_tls_config(&tls_config.cert_path, &tls_config.key_path)
            .map_err(|e| anyhow::anyhow!("Failed to create TLS config: {}", e))?;

        // Bind to address
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let actual_addr = listener.local_addr()?;
        self.port = actual_addr.port();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Convert to std::net::TcpListener for axum-server
        let std_listener = listener.into_std()?;

        // Start HTTPS server using axum-server
        let server_handle = tokio::spawn(async move {
            let server = axum_server::from_tcp_rustls(std_listener, rustls_config)
                .serve(router.into_make_service());

            tokio::select! {
                result = server => {
                    if let Err(e) = result {
                        error!("HTTPS server error: {}", e);
                    }
                }
                _ = shutdown_rx => {
                    debug!("HTTPS server received shutdown signal");
                }
            }
        });

        self.server_handle = Some(server_handle);
        self.running = true;

        info!("HTTPS server {} started on {}:{}", self.id, host, self.port);
        Ok(self.port)
    }

    pub async fn stop(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.server_handle.take() {
            handle.abort();

            let connections_count = self.active_ws_connections.read().await.len();
            if connections_count > 0 {
                debug!(
                    "Closing {} WebSocket connections for server {}",
                    connections_count, self.id
                );
                let mut connections = self.active_ws_connections.write().await;
                connections.clear();
            }
        }

        self.running = false;
        info!("HTTP server {} on port {} stopped", self.id, self.port);

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(())
    }

    fn build_router_safe(&self, actor_handle: ActorHandle) -> Result<Router, String> {
        let result =
            panic::catch_unwind(panic::AssertUnwindSafe(|| self.build_router(actor_handle)));

        match result {
            Ok(router) => Ok(router),
            Err(panic_payload) => {
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "Unknown panic occurred while building router".to_string()
                };

                let error_msg = if panic_msg.contains("Path segments must not start with `*`") {
                    let suggestion = "Path segments must not start with `*`. For wildcard capture, use `{*wildcard}` syntax instead.";
                    format!("Invalid route pattern: {}. {}", panic_msg, suggestion)
                } else if panic_msg.contains("without_v07_checks") {
                    format!("Route pattern validation failed: {}. Please update route patterns to use Axum v0.7+ syntax.", panic_msg)
                } else {
                    format!("Router building failed: {}", panic_msg)
                };

                Err(error_msg)
            }
        }
    }
}

impl ServerState {
    pub fn find_middlewares(&self, path: &str) -> Vec<&MiddlewareConfig> {
        let mut result: Vec<&MiddlewareConfig> = self
            .middlewares
            .values()
            .filter(|middleware| {
                // Match if the middleware path is a prefix of the request path
                // or is exactly the request path or is catch-all
                path.starts_with(&middleware.path) || middleware.path == "/*"
            })
            .collect();

        result.sort_by(|a, b| a.priority.cmp(&b.priority));
        result
    }

    pub async fn call_route_handler(
        &self,
        handler_id: u64,
        request: HttpRequest,
    ) -> Result<HttpResponse> {
        let result = self
            .actor_handle
            .call_function::<(u64, HttpRequest), (HttpResponse,)>(
                "theater:simple/http-handlers.handle-request".to_string(),
                (handler_id, request),
            )
            .await?;

        Ok(result.0)
    }

    pub async fn call_middleware(
        &self,
        handler_id: u64,
        request: HttpRequest,
    ) -> Result<MiddlewareResult> {
        let result = self
            .actor_handle
            .call_function::<(u64, HttpRequest), (MiddlewareResult,)>(
                "theater:simple/http-handlers.handle-middleware".to_string(),
                (handler_id, request),
            )
            .await?;

        Ok(result.0)
    }
}

// Route pattern validation helper
fn validate_route_pattern(path: &str) -> Result<(), String> {
    // Basic validation for common Axum v0.7+ pattern issues
    if path.contains("/*") && !path.contains("/{*") {
        return Err(format!(
            "Invalid wildcard pattern '{}'. Use '{{*wildcard}}' instead of '*' for wildcard capture.",
            path
        ));
    }

    // Check for other common pattern issues
    if path.contains("{") && !path.contains("}") {
        return Err(format!("Unmatched '{{' in route pattern: {}", path));
    }

    if path.contains("}") && !path.contains("{") {
        return Err(format!("Unmatched '}}' in route pattern: {}", path));
    }

    Ok(())
}

// NEW: Enhanced request conversion that includes Axum-extracted path parameters
async fn convert_request_with_axum_params(
    req: Request<axum::body::Body>,
    path_params: Option<Path<HashMap<String, String>>>,
) -> HttpRequest {
    let method = req.method().as_str().to_string();
    let uri = req.uri().to_string();

    // Extract headers
    let mut headers = Vec::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.push((name.to_string(), value_str.to_string()));
        }
    }

    // Add path parameters as special headers so WASM handlers can access them
    if let Some(Path(params)) = path_params {
        for (key, value) in params {
            headers.push((format!("X-Route-Param-{}", key), value.clone()));
            debug!("Extracted path parameter: {} = {}", key, value);
        }
    }

    // Read body
    let body = match axum::body::to_bytes(req.into_body(), 100 * 1024 * 1024).await {
        Ok(bytes) => Some(bytes.to_vec()),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            None
        }
    };

    HttpRequest {
        method,
        uri,
        headers,
        body,
    }
}

// Helper function to convert Theater response to Axum response
fn convert_response(response: HttpResponse) -> Response<axum::body::Body> {
    let mut builder = Response::builder().status(response.status);

    // Add headers
    for (name, value) in response.headers {
        if let (Ok(header_name), Ok(header_value)) =
            (HeaderName::try_from(name), HeaderValue::try_from(value))
        {
            builder = builder.header(header_name, header_value);
        }
    }

    // Set body
    let body = response.body.unwrap_or_default();
    builder
        .body(axum::body::Body::from(body))
        .unwrap_or_default()
}
