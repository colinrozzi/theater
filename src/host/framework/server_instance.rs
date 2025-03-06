use crate::actor_handle::ActorHandle;
use anyhow::{anyhow, Result};
use axum::{
    extract::{State, WebSocketUpgrade},
    http::{HeaderName, HeaderValue, Request, StatusCode},
    response::Response,
    routing::{any, get},
    Router,
};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{error, info};

use super::types::*;

// Route configuration
#[derive(Clone)]
pub struct RouteConfig {
    pub id: u64,
    pub path: String,
    pub method: String,
    pub handler_id: u64,
}

// Middleware configuration
#[derive(Clone)]
pub struct MiddlewareConfig {
    pub id: u64,
    pub path: String,
    pub handler_id: u64,
    pub priority: u32, // Lower numbers run first
}

// WebSocket configuration
#[derive(Clone)]
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

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn has_route(&self, route_id: u64) -> bool {
        self.routes.contains_key(&route_id)
    }

    pub fn has_middleware(&self, middleware_id: u64) -> bool {
        self.middlewares.contains_key(&middleware_id)
    }

    pub async fn add_route(
        &mut self,
        route_id: u64,
        path: String,
        method: String,
        handler_id: u64,
    ) -> Result<()> {
        // Validate path and method
        if !path.starts_with('/') {
            return Err(anyhow!("Path must start with /"));
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
        if self.routes.remove(&route_id).is_none() {
            return Err(anyhow!("Route not found: {}", route_id));
        }
        Ok(())
    }

    pub fn add_middleware(
        &mut self,
        middleware_id: u64,
        path: String,
        handler_id: u64,
    ) -> Result<()> {
        // Validate path
        if !path.starts_with('/') {
            return Err(anyhow!("Path must start with /"));
        }

        // Calculate priority based on path specificity
        // More specific paths (longer) should run later
        let priority = path.len() as u32;

        let middleware_config = MiddlewareConfig {
            id: middleware_id,
            path,
            handler_id,
            priority,
        };

        self.middlewares.insert(middleware_id, middleware_config);
        Ok(())
    }

    pub fn remove_middleware(&mut self, middleware_id: u64) -> Result<()> {
        if self.middlewares.remove(&middleware_id).is_none() {
            return Err(anyhow!("Middleware not found: {}", middleware_id));
        }
        Ok(())
    }

    pub fn enable_websocket(
        &mut self,
        path: String,
        connect_handler_id: Option<u64>,
        message_handler_id: u64,
        disconnect_handler_id: Option<u64>,
    ) -> Result<()> {
        // Validate path
        if !path.starts_with('/') {
            return Err(anyhow!("Path must start with /"));
        }

        let websocket_config = WebSocketConfig {
            path: path.clone(),
            connect_handler_id,
            message_handler_id,
            disconnect_handler_id,
        };

        self.websockets.insert(path, websocket_config);
        Ok(())
    }

    pub fn disable_websocket(&mut self, path: &str) -> Result<()> {
        if self.websockets.remove(path).is_none() {
            return Err(anyhow!("WebSocket not enabled on path: {}", path));
        }
        Ok(())
    }

    pub async fn start(&mut self, actor_handle: ActorHandle) -> Result<u16> {
        if self.running {
            return Ok(self.port);
        }

        // Build router based on current configuration
        let router = self.build_router(actor_handle.clone());

        // Create address
        let host = self
            .config
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        // Use port 0 to let the OS assign a free port if none specified
        let port = self.config.port.unwrap_or(0);
        let addr = format!("{}:{}", host, port).parse::<SocketAddr>()?;

        // Start listener
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        let actual_port = actual_addr.port();
        self.port = actual_port;

        // Launch server in separate task
        let server_handle = tokio::spawn(async move {
            info!("Server starting on {}", actual_addr);

            // Use Axum's serve function with our router
            let server = axum::serve(listener, router);
            if let Err(e) = server.await {
                error!("Server error: {}", e);
            }

            info!("Server stopped");
        });

        self.server_handle = Some(server_handle);
        self.running = true;

        info!("HTTP server started on port {}", actual_port);
        Ok(actual_port)
    }

    pub async fn stop(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }

        // Cancel the server task
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
            // We don't need to wait for it to complete
            // It will be properly cancelled
        }

        // No need to close listener - it's owned by the server task

        // Close all WebSocket connections
        let mut connections = self.active_ws_connections.write().await;
        connections.clear();

        self.running = false;
        info!("HTTP server stopped");

        Ok(())
    }

    fn build_router(&self, actor_handle: ActorHandle) -> Router {
        // Start with an empty router
        let mut router = Router::new();

        // Create shared state for all handlers
        let state = Arc::new(ServerState {
            id: self.id,
            actor_handle,
            routes: self.routes.clone(),
            middlewares: self.middlewares.clone(),
            websockets: self.websockets.clone(),
            active_ws_connections: self.active_ws_connections.clone(),
        });

        // Add routes
        let mut route_paths = HashMap::<String, HashMap<String, RouteConfig>>::new();

        // Group routes by path
        for route in self.routes.values() {
            route_paths
                .entry(route.path.clone())
                .or_insert_with(HashMap::new)
                .insert(route.method.clone(), route.clone());
        }

        let route_paths_clone = route_paths.clone();

        // Add each path with its methods
        for (path, methods) in route_paths_clone {
            // Check if this path has a WebSocket handler
            let has_ws = self.websockets.contains_key(&path);

            // For each path, collect the appropriate HTTP methods
            let mut get_methods = Vec::new();
            let mut post_methods = Vec::new();
            let mut put_methods = Vec::new();
            let mut delete_methods = Vec::new();
            let mut patch_methods = Vec::new();
            let mut options_methods = Vec::new();
            let mut head_methods = Vec::new();
            let mut other_methods = Vec::new();

            for (method_name, route_config) in methods {
                match method_name.as_str() {
                    "GET" => get_methods.push(route_config),
                    "POST" => post_methods.push(route_config),
                    "PUT" => put_methods.push(route_config),
                    "DELETE" => delete_methods.push(route_config),
                    "PATCH" => patch_methods.push(route_config),
                    "OPTIONS" => options_methods.push(route_config),
                    "HEAD" => head_methods.push(route_config),
                    _ => other_methods.push(route_config),
                }
            }

            // Build router for this path
            let mut path_router = Router::new();

            // Add each method handler
            if !get_methods.is_empty() {
                path_router = path_router.route(&path, get(Self::handle_http_request));
            }

            if !post_methods.is_empty() {
                path_router =
                    path_router.route(&path, axum::routing::post(Self::handle_http_request));
            }

            if !put_methods.is_empty() {
                path_router =
                    path_router.route(&path, axum::routing::put(Self::handle_http_request));
            }

            if !delete_methods.is_empty() {
                path_router =
                    path_router.route(&path, axum::routing::delete(Self::handle_http_request));
            }

            if !patch_methods.is_empty() {
                path_router =
                    path_router.route(&path, axum::routing::patch(Self::handle_http_request));
            }

            if !options_methods.is_empty() {
                path_router =
                    path_router.route(&path, axum::routing::options(Self::handle_http_request));
            }

            if !head_methods.is_empty() {
                path_router =
                    path_router.route(&path, axum::routing::head(Self::handle_http_request));
            }

            if !other_methods.is_empty() {
                path_router = path_router.route(&path, any(Self::handle_http_request));
            }

            // Add WebSocket handler if needed
            if has_ws {
                path_router = path_router.route(&path, get(Self::handle_websocket_upgrade));
            }

            // Merge with main router
            router = router.merge(path_router.with_state(state.clone()));
        }

        // Add WebSocket paths that don't have HTTP handlers
        for (ws_path, _) in &self.websockets {
            if !route_paths.contains_key(ws_path) {
                router = router.route(
                    ws_path,
                    get(Self::handle_websocket_upgrade).with_state(state.clone()),
                );
            }
        }

        router
    }

    async fn handle_http_request(
        State(state): State<Arc<ServerState>>,
        req: Request<axum::body::Body>,
    ) -> Response<axum::body::Body> {
        // Extract path and method
        let path = req.uri().path().to_string();
        let method = req.method().as_str().to_uppercase();

        // Find matching route
        let route = state.find_route(&path, &method);

        if let Some(route) = route {
            // Convert Axum request to our HTTP request format
            let mut theater_request = convert_request(req).await;

            // Apply middlewares
            let middlewares = state.find_middlewares(&path);
            for middleware in middlewares {
                match state
                    .call_middleware(middleware.handler_id, theater_request.clone())
                    .await
                {
                    Ok(result) => {
                        if !result.proceed {
                            // Middleware rejected the request
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

            // Call the route handler
            match state
                .call_route_handler(route.handler_id, theater_request)
                .await
            {
                Ok(response) => convert_response(response),
                Err(e) => {
                    error!("Handler error: {}", e);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(format!("Handler error: {}", e).into())
                        .unwrap_or_default()
                }
            }
        } else {
            // No matching route
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::empty())
                .unwrap_or_default()
        }
    }

    async fn handle_websocket_upgrade(
        State(state): State<Arc<ServerState>>,
        ws: WebSocketUpgrade,
        req: Request<axum::body::Body>,
    ) -> Response<axum::body::Body> {
        // Extract path
        let path = req.uri().path().to_string();

        // Find WebSocket configuration for this path
        if let Some(config) = state.websockets.get(&path).cloned() {
            // Generate a connection ID
            let connection_id = rand::random::<u64>();

            // Extract query parameters
            let query = req.uri().query().map(|q| q.to_string());

            // Clone the path and state for use in the closure
            let path_clone = path.clone();
            let state_clone = state.clone();

            // Upgrade the connection
            ws.on_upgrade(move |socket| async move {
                // Handle the WebSocket connection
                Self::handle_websocket_connection(
                    state_clone,
                    socket,
                    connection_id,
                    path_clone,
                    query,
                    config,
                )
                .await;
            })
        } else {
            // No WebSocket configured for this path
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body("WebSocket not available on this path".into())
                .unwrap_or_default()
        }
    }

    async fn handle_websocket_connection(
        state: Arc<ServerState>,
        socket: axum::extract::ws::WebSocket,
        connection_id: u64,
        path: String,
        query: Option<String>,
        config: WebSocketConfig,
    ) {
        let (mut ws_sender, mut ws_receiver) = socket.split();

        // Create a channel for sending messages back to the WebSocket
        let (sender, mut receiver) = mpsc::channel::<WebSocketMessage>(100);

        // Store the connection
        {
            let mut connections = state.active_ws_connections.write().await;
            connections.insert(
                connection_id,
                WebSocketConnection {
                    id: connection_id,
                    sender: sender.clone(),
                },
            );
        }

        // Call connect handler if configured
        if let Some(handler_id) = config.connect_handler_id {
            if let Err(e) = state
                .call_websocket_connect(handler_id, connection_id, path.clone(), query.clone())
                .await
            {
                error!("WebSocket connect handler error: {}", e);
            }
        }

        // Spawn task to forward messages to WebSocket
        let forward_task = tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message.ty {
                    MessageType::Text => {
                        if let Some(text) = message.text {
                            if let Err(e) = ws_sender
                                .send(axum::extract::ws::Message::Text(text.into()))
                                .await
                            {
                                error!("Error sending WebSocket text message: {}", e);
                                break;
                            }
                        }
                    }
                    MessageType::Binary => {
                        if let Some(data) = message.data {
                            if let Err(e) = ws_sender
                                .send(axum::extract::ws::Message::Binary(data.into()))
                                .await
                            {
                                error!("Error sending WebSocket binary message: {}", e);
                                break;
                            }
                        }
                    }
                    MessageType::Close => {
                        let _ = ws_sender.close().await;
                        break;
                    }
                    MessageType::Ping => {
                        if let Err(e) = ws_sender
                            .send(axum::extract::ws::Message::Ping(vec![].into()))
                            .await
                        {
                            error!("Error sending WebSocket ping: {}", e);
                            break;
                        }
                    }
                    MessageType::Pong => {
                        if let Err(e) = ws_sender
                            .send(axum::extract::ws::Message::Pong(vec![].into()))
                            .await
                        {
                            error!("Error sending WebSocket pong: {}", e);
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        // Process incoming messages
        while let Some(result) = ws_receiver.next().await {
            match result {
                Ok(message) => {
                    let theater_message = match message {
                        axum::extract::ws::Message::Text(text) => WebSocketMessage {
                            ty: MessageType::Text,
                            data: None,
                            text: Some(text.to_string()),
                        },
                        axum::extract::ws::Message::Binary(data) => WebSocketMessage {
                            ty: MessageType::Binary,
                            data: Some(data.to_vec()),
                            text: None,
                        },
                        axum::extract::ws::Message::Close(_) => {
                            // Call disconnect handler
                            if let Some(handler_id) = config.disconnect_handler_id {
                                if let Err(e) = state
                                    .call_websocket_disconnect(handler_id, connection_id)
                                    .await
                                {
                                    error!("WebSocket disconnect handler error: {}", e);
                                }
                            }

                            break;
                        }
                        axum::extract::ws::Message::Ping(_) => WebSocketMessage {
                            ty: MessageType::Ping,
                            data: None,
                            text: None,
                        },
                        axum::extract::ws::Message::Pong(_) => WebSocketMessage {
                            ty: MessageType::Pong,
                            data: None,
                            text: None,
                        },
                    };

                    // Call message handler
                    match state
                        .call_websocket_message(
                            config.message_handler_id,
                            connection_id,
                            theater_message,
                        )
                        .await
                    {
                        Ok(responses) => {
                            // Forward any response messages back
                            for response in responses {
                                if let Err(e) = sender.send(response).await {
                                    error!("Error sending response to WebSocket: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("WebSocket message handler error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        // Clean up
        forward_task.abort();

        // Remove the connection
        {
            let mut connections = state.active_ws_connections.write().await;
            connections.remove(&connection_id);
        }

        // Call disconnect handler if not already called
        if let Some(handler_id) = config.disconnect_handler_id {
            if let Err(e) = state
                .call_websocket_disconnect(handler_id, connection_id)
                .await
            {
                error!("WebSocket disconnect handler error: {}", e);
            }
        }
    }
}

// Helper struct for router state
pub struct ServerState {
    pub id: u64,
    pub actor_handle: ActorHandle,
    pub routes: HashMap<u64, RouteConfig>,
    pub middlewares: HashMap<u64, MiddlewareConfig>,
    pub websockets: HashMap<String, WebSocketConfig>,
    pub active_ws_connections: Arc<RwLock<HashMap<u64, WebSocketConnection>>>,
}

impl ServerState {
    pub fn find_route(&self, path: &str, method: &str) -> Option<&RouteConfig> {
        self.routes
            .values()
            .find(|route| route.path == path && (route.method == method || route.method == "*"))
    }

    pub fn find_middlewares(&self, path: &str) -> Vec<&MiddlewareConfig> {
        let mut result: Vec<&MiddlewareConfig> = self
            .middlewares
            .values()
            .filter(|middleware| {
                // Match if the middleware path is a prefix of the request path
                // or is exactly the request path
                path.starts_with(&middleware.path) || middleware.path == "/*"
            })
            .collect();

        // Sort by priority
        result.sort_by(|a, b| a.priority.cmp(&b.priority));

        result
    }

    pub async fn call_route_handler(
        &self,
        handler_id: u64,
        request: HttpRequest,
    ) -> Result<HttpResponse> {
        // Find handler function name
        let result = self
            .actor_handle
            .call_function::<(u64, HttpRequest), (HttpResponse,)>(
                "ntwk:theater/http-handlers.handle-request".to_string(),
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
        // Call middleware function
        let result = self
            .actor_handle
            .call_function::<(u64, HttpRequest), (MiddlewareResult,)>(
                "ntwk:theater/http-handlers.handle-middleware".to_string(),
                (handler_id, request),
            )
            .await?;

        Ok(result.0)
    }

    pub async fn call_websocket_connect(
        &self,
        handler_id: u64,
        connection_id: u64,
        path: String,
        query: Option<String>,
    ) -> Result<()> {
        // Call connect handler
        self.actor_handle
            .call_function::<(u64, u64, String, Option<String>), ()>(
                "ntwk:theater/http-handlers.handle-websocket-connect".to_string(),
                (handler_id, connection_id, path, query),
            )
            .await?;

        Ok(())
    }

    pub async fn call_websocket_message(
        &self,
        handler_id: u64,
        connection_id: u64,
        message: WebSocketMessage,
    ) -> Result<Vec<WebSocketMessage>> {
        // Call message handler
        let result = self
            .actor_handle
            .call_function::<(u64, u64, WebSocketMessage), (Vec<WebSocketMessage>,)>(
                "ntwk:theater/http-handlers.handle-websocket-message".to_string(),
                (handler_id, connection_id, message),
            )
            .await?;

        Ok(result.0)
    }

    pub async fn call_websocket_disconnect(
        &self,
        handler_id: u64,
        connection_id: u64,
    ) -> Result<()> {
        // Call disconnect handler
        self.actor_handle
            .call_function::<(u64, u64), ()>(
                "ntwk:theater/http-handlers.handle-websocket-disconnect".to_string(),
                (handler_id, connection_id),
            )
            .await?;

        Ok(())
    }
}

// Helper functions to convert between Axum and Theater types
async fn convert_request(req: Request<axum::body::Body>) -> HttpRequest {
    // Split into parts to access headers and body separately
    let (parts, body) = req.into_parts();

    // Extract headers
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();

    // Read body
    let body_bytes = match axum::body::to_bytes(body, 100 * 1024 * 1024).await {
        Ok(bytes) => Some(bytes.to_vec()),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            None
        }
    };

    HttpRequest {
        method: parts.method.as_str().to_string(),
        uri: parts.uri.to_string(),
        headers,
        body: body_bytes,
    }
}

fn convert_response(response: HttpResponse) -> Response<axum::body::Body> {
    // Create response builder with status
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);

    // Add headers
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in response.headers {
            if let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) {
                if let Ok(header_value) = HeaderValue::from_str(&value) {
                    headers.insert(header_name, header_value);
                }
            }
        }
    }

    // Create response with body
    if let Some(body) = response.body {
        builder
            .body(axum::body::Body::from(body))
            .unwrap_or_default()
    } else {
        builder.body(axum::body::Body::empty()).unwrap_or_default()
    }
}
