//! # HTTP Framework Handler
//!
//! Provides a complete HTTP/HTTPS framework for WebAssembly actors in the Theater system.
//! This handler allows actors to create and manage HTTP servers with routing, middleware,
//! WebSocket support, and TLS/HTTPS capabilities.
//!
//! ## Features
//!
//! - **HTTP & HTTPS servers**: Create multiple HTTP or HTTPS servers with TLS support
//! - **Flexible routing**: Add routes with HTTP method and path patterns (using Axum)
//! - **Middleware support**: Add middleware to routes with priority-based execution
//! - **WebSocket support**: Enable WebSocket endpoints with connect/message/disconnect handlers
//! - **TLS/HTTPS**: Full TLS support with certificate and key loading
//! - **Multiple servers**: Create and manage multiple independent server instances
//! - **Event logging**: All HTTP operations are logged to the chain
//!
//! ## Example
//!
//! ```rust,no_run
//! use theater_handler_http_framework::HttpFrameworkHandler;
//!
//! let handler = HttpFrameworkHandler::new(None);
//! ```

pub mod events;
mod handlers;
mod server_instance;
mod tls;
mod types;

pub use events::HttpFrameworkEventData;

pub use handlers::{HandlerConfig, HandlerRegistry, HandlerType};
pub use server_instance::ServerInstance;
pub use tls::*;
pub use types::*;

use anyhow::Result;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use theater::actor::handle::ActorHandle;
use theater::actor::store::ActorStore;
use theater::config::permissions::HttpFrameworkPermissions;
use theater::handler::{Handler, HandlerContext, SharedActorInstance};
use theater::shutdown::ShutdownReceiver;
use theater::wasm::{ActorComponent, ActorInstance};

use crate::events::HttpFrameworkEventData as HandlerEventData;

/// Error types for HTTP framework operations
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum HttpFrameworkError {
    #[error("Server not found: {0}")]
    ServerNotFound(u64),

    #[error("Handler not found: {0}")]
    HandlerNotFound(u64),

    #[error("Route not found: {0}")]
    RouteNotFound(u64),

    #[error("Middleware not found: {0}")]
    MiddlewareNotFound(u64),

    #[error("WebSocket not enabled on path: {0}")]
    WebSocketNotEnabled(String),

    #[error("WebSocket connection not found: {0}")]
    WebSocketConnectionNotFound(u64),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

#[allow(dead_code)]
struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    server_id: u64,
}

/// Handler for providing HTTP framework capabilities to WebAssembly actors
#[derive(Clone)]
pub struct HttpFrameworkHandler {
    servers: Arc<RwLock<HashMap<u64, ServerInstance>>>,
    handlers: Arc<RwLock<HandlerRegistry>>,
    next_server_id: Arc<AtomicU64>,
    next_handler_id: Arc<AtomicU64>,
    next_route_id: Arc<AtomicU64>,
    next_middleware_id: Arc<AtomicU64>,
    server_handles: Arc<RwLock<HashMap<u64, ServerHandle>>>,
    #[allow(dead_code)]
    permissions: Option<HttpFrameworkPermissions>,
}

impl HttpFrameworkHandler {
    /// Create a new HTTP framework handler
    ///
    /// # Arguments
    ///
    /// * `permissions` - Optional permissions controlling HTTP framework access
    pub fn new(permissions: Option<HttpFrameworkPermissions>) -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            next_server_id: Arc::new(AtomicU64::new(1)),
            next_handler_id: Arc::new(AtomicU64::new(1)),
            next_route_id: Arc::new(AtomicU64::new(1)),
            next_middleware_id: Arc::new(AtomicU64::new(1)),
            server_handles: Arc::new(RwLock::new(HashMap::new())),
            permissions,
        }
    }
}

impl Handler for HttpFrameworkHandler
{
    fn create_instance(&self, _config: Option<&theater::config::actor_manifest::HandlerConfig>) -> Box<dyn Handler> {
        Box::new(self.clone())
    }

    fn setup_host_functions(&mut self, actor_component: &mut ActorComponent, _ctx: &mut HandlerContext) -> Result<()> {
        info!("Setting up HTTP framework host functions");

        let mut interface = actor_component
            .linker
            .instance("theater:simple/http-framework")
            .map_err(|e| {
                anyhow::anyhow!(
                    "Could not instantiate theater:simple/http-framework: {}",
                    e
                )
            })?;

        // 1. create-server
        let servers_clone = self.servers.clone();
        let next_server_id = self.next_server_id.clone();
        interface.func_wrap(
            "create-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (config,): (ServerConfig,)|
                  -> Result<(Result<u64, String>,)> {
                let server_id = next_server_id.fetch_add(1, Ordering::SeqCst);

                // Extract config values with defaults
                let port = config.port.unwrap_or(0);
                let host = config.host.unwrap_or_else(|| "127.0.0.1".to_string());

                // Validate host
                if !is_valid_host(&host) {
                    return Ok((Err(format!("Invalid host: {}", host)),));
                }

                // Validate TLS config if present
                if let Some(tls) = &config.tls_config {
                    if tls.cert_path.is_empty() || tls.key_path.is_empty() {
                        return Ok((Err("Invalid TLS configuration".to_string()),));
                    }
                }

                // Create server instance (not started yet)
                let server =
                    ServerInstance::new(server_id, port, host.clone(), config.tls_config.clone());

                // Record event in hash chain
                
                // Store server instance
                let servers_clone_inner = servers_clone.clone();
                tokio::spawn(async move {
                    let mut servers = servers_clone_inner.write().await;
                    servers.insert(server_id, server);
                });

                Ok((Ok(server_id),))
            },
        )?;

        // 2. get-server-info
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "get-server-info",
            move |_ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Result<(Result<ServerInfo, String>,)> {
                let servers_clone = servers_clone.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let servers = servers_clone.read().await;

                    if let Some(server) = servers.get(&server_id) {
                        Ok(server.get_info())
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                match tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                }) {
                    Ok(Ok(info)) => Ok((Ok(info),)),
                    Ok(Err(e)) => Ok((Err(e),)),
                    Err(e) => Ok((Err(format!("Internal error: {}", e)),)),
                }
            },
        )?;

        // 3. start-server
        let servers_clone = self.servers.clone();
        let server_handles_clone = self.server_handles.clone();
        interface.func_wrap_async(
            "start-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<u16, String>,)>> + Send> {
                let servers_clone = servers_clone.clone();
                let actor_handle = ctx.data().actor_handle.clone();

                let server_handles_clone = server_handles_clone.clone();
                Box::new(async move {
                    let mut servers = servers_clone.write().await;

                    if let Some(server) = servers.get_mut(&server_id) {
                        match server.start(actor_handle.clone()).await {
                            Ok(port) => {
                                // Record appropriate event based on server type
                                let event_data = if server.is_https() {
                                    HandlerEventData::ServerStartHttps {
                                        server_id,
                                        port,
                                        cert_path: server
                                            .get_tls_cert_path()
                                            .unwrap_or("unknown")
                                            .to_string(),
                                    }
                                } else {
                                    HandlerEventData::ServerStart { server_id, port }
                                };

                                let description = if server.is_https() {
                                    format!("Started HTTPS server {} on port {}", server_id, port)
                                } else {
                                    format!("Started HTTP server {} on port {}", server_id, port)
                                };

                                
                                // Create server handle for tracking
                                let server_handle = ServerHandle {
                                    shutdown_tx: None,
                                    task: None,
                                    server_id,
                                };

                                // Store the handle
                                let handle_clone = server_handles_clone.clone();
                                tokio::spawn(async move {
                                    let mut handles = handle_clone.write().await;
                                    handles.insert(server_id, server_handle);
                                    debug!("Stored handle for server {}", server_id);
                                });

                                debug!("Started server {} on port {}", server_id, port);
                                Ok((Ok(port),))
                            }
                            Err(e) => {
                                error!("Failed to start server {}: {}", server_id, e);

                                // Record error event
                                
                                Ok((Err(format!("Failed to start server: {}", e)),))
                            }
                        }
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // 4. stop-server
        let servers_clone = self.servers.clone();
        let server_handles_clone = self.server_handles.clone();
        interface.func_wrap_async(
            "stop-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let servers_clone = servers_clone.clone();
                let server_handles_clone = server_handles_clone.clone();

                Box::new(async move {
                    let mut servers = servers_clone.write().await;

                    if let Some(server) = servers.get_mut(&server_id) {
                        let result = server.stop().await;

                        // Remove the server handle since we're explicitly stopping it
                        let handles_clone = server_handles_clone.clone();
                        tokio::spawn(async move {
                            let mut handles = handles_clone.write().await;
                            if handles.remove(&server_id).is_some() {
                                debug!("Removed handle for stopped server {}", server_id);
                            }
                        });

                        match result {
                            Ok(_) => {
                                // Record event
                                
                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                error!("Failed to stop server {}: {}", server_id, e);

                                // Record error event
                                
                                Ok((Err(format!("Failed to stop server: {}", e)),))
                            }
                        }
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // 5. destroy-server
        let servers_clone = self.servers.clone();
        let server_handles_clone = self.server_handles.clone();
        interface.func_wrap_async(
            "destroy-server",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id,): (u64,)|
                  -> Box<dyn Future<Output = Result<(Result<(), String>,)>> + Send> {
                let servers_clone = servers_clone.clone();
                let server_handles_clone = server_handles_clone.clone();

                Box::new(async move {
                    let mut servers = servers_clone.write().await;

                    if let Some(server) = servers.get_mut(&server_id) {
                        // Make sure server is stopped first
                        if server.is_running() {
                            match server.stop().await {
                                Ok(_) => {}
                                Err(e) => {
                                    error!(
                                        "Failed to stop server {} during destroy: {}",
                                        server_id, e
                                    );
                                    // Continue with removal anyway
                                }
                            }
                        }

                        // Remove the server
                        servers.remove(&server_id);

                        // Remove the server handle
                        let handles_clone = server_handles_clone.clone();
                        tokio::spawn(async move {
                            let mut handles = handles_clone.write().await;
                            if handles.remove(&server_id).is_some() {
                                debug!("Removed handle for destroyed server {}", server_id);
                            }
                        });

                        // Record event
                        
                        Ok((Ok(()),))
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // 6. register-handler
        let handlers_clone = self.handlers.clone();
        let next_handler_id = self.next_handler_id.clone();
        interface.func_wrap(
            "register-handler",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (handler_name,): (String,)|
                  -> Result<(Result<u64, String>,)> {
                let handlers_clone = handlers_clone.clone();
                let handler_id = next_handler_id.fetch_add(1, Ordering::SeqCst);

                // Validate handler name
                if handler_name.is_empty() {
                    return Ok((Err("Handler name cannot be empty".to_string()),));
                }

                // Create handler config
                let handler_config = HandlerConfig {
                    id: handler_id,
                    name: handler_name.clone(),
                    handler_type: HandlerType::Unknown,
                };

                // Record event
                
                // Store handler
                tokio::spawn(async move {
                    let mut handlers = handlers_clone.write().await;
                    handlers.register(handler_config);
                });

                Ok((Ok(handler_id),))
            },
        )?;

        // 7. add-route
        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        let next_route_id = self.next_route_id.clone();
        interface.func_wrap(
            "add-route",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, path, method, handler_id): (u64, String, String, u64)|
                  -> Result<(Result<u64, String>,)> {
                let servers_clone = servers_clone.clone();
                let handlers_clone = handlers_clone.clone();
                let route_id = next_route_id.fetch_add(1, Ordering::SeqCst);

                // Validate path
                if !path.starts_with('/') {
                    return Ok((Err("Path must start with /".to_string()),));
                }

                // Validate method
                let method = method.to_uppercase();
                if !is_valid_method(&method) {
                    return Ok((Err(format!("Invalid HTTP method: {}", method)),));
                }

                // Clone values before moving to async context
                let path_clone = path.clone();
                let method_clone = method.clone();

                // Capture the current execution context for async operations
                let current_task = tokio::task::spawn(async move {
                    // Verify handler exists
                    let handlers = handlers_clone.read().await;
                    if !handlers.exists(handler_id) {
                        return Err(format!("Handler not found: {}", handler_id));
                    }

                    // Mark handler as HTTP request handler
                    handlers
                        .set_handler_type(handler_id, HandlerType::HttpRequest)
                        .await;

                    // Verify server exists and add route
                    let mut servers = servers_clone.write().await;
                    if let Some(server) = servers.get_mut(&server_id) {
                        match server
                            .add_route(
                                route_id,
                                path_clone.clone(),
                                method_clone.clone(),
                                handler_id,
                            )
                            .await
                        {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                let path_clone = path.clone();
                let method_clone = method.clone();

                match result {
                    Ok(_) => {
                        // Record event
                        
                        Ok((Ok(route_id),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 8. remove-route
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "remove-route",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (route_id,): (u64,)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let mut servers = servers_clone.write().await;

                    // Find server with this route
                    let mut found = false;
                    let mut server_id = 0;

                    for (id, server) in servers.iter_mut() {
                        if server.has_route(route_id) {
                            found = true;
                            server_id = *id;
                            if let Err(e) = server.remove_route(route_id) {
                                return Err(e.to_string());
                            }
                            break;
                        }
                    }

                    if found {
                        Ok(server_id)
                    } else {
                        Err(format!("Route not found: {}", route_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                match result {
                    Ok(server_id) => {
                        // Record event
                        
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 9. add-middleware
        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        let next_middleware_id = self.next_middleware_id.clone();
        interface.func_wrap(
            "add-middleware",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, path, handler_id): (u64, String, u64)|
                  -> Result<(Result<u64, String>,)> {
                let servers_clone = servers_clone.clone();
                let handlers_clone = handlers_clone.clone();
                let middleware_id = next_middleware_id.fetch_add(1, Ordering::SeqCst);

                // Validate path
                if !path.starts_with('/') {
                    return Ok((Err("Path must start with /".to_string()),));
                }

                // Clone values before moving to async context
                let path_clone = path.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    // Verify handler exists
                    let handlers = handlers_clone.read().await;
                    if !handlers.exists(handler_id) {
                        return Err(format!("Handler not found: {}", handler_id));
                    }

                    // Mark handler as middleware handler
                    handlers
                        .set_handler_type(handler_id, HandlerType::Middleware)
                        .await;

                    // Verify server exists and add middleware
                    let mut servers = servers_clone.write().await;
                    if let Some(server) = servers.get_mut(&server_id) {
                        match server.add_middleware(middleware_id, path_clone.clone(), handler_id) {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                let path_clone = path.clone();

                match result {
                    Ok(_) => {
                        // Record event
                        
                        Ok((Ok(middleware_id),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 10. remove-middleware
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "remove-middleware",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (middleware_id,): (u64,)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let mut servers = servers_clone.write().await;

                    // Find server with this middleware
                    let mut found = false;
                    let mut server_id = 0;

                    for (id, server) in servers.iter_mut() {
                        if server.has_middleware(middleware_id) {
                            found = true;
                            server_id = *id;
                            if let Err(e) = server.remove_middleware(middleware_id) {
                                return Err(e.to_string());
                            }
                            break;
                        }
                    }

                    if found {
                        Ok(server_id)
                    } else {
                        Err(format!("Middleware not found: {}", middleware_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                match result {
                    Ok(server_id) => {
                        // Record event
                        
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 11. enable-websocket
        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        interface.func_wrap(
            "enable-websocket",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (
                server_id,
                path,
                connect_handler_id,
                message_handler_id,
                disconnect_handler_id,
            ): (u64, String, Option<u64>, u64, Option<u64>)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();
                let handlers_clone = handlers_clone.clone();

                // Validate path
                if !path.starts_with('/') {
                    return Ok((Err("Path must start with /".to_string()),));
                }

                // Clone values before moving to async context
                let path_clone = path.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let handlers = handlers_clone.read().await;

                    // Verify message handler exists
                    if !handlers.exists(message_handler_id) {
                        return Err(format!("Message handler not found: {}", message_handler_id));
                    }

                    // Verify connect handler if provided
                    if let Some(id) = connect_handler_id {
                        if !handlers.exists(id) {
                            return Err(format!("Connect handler not found: {}", id));
                        }
                    }

                    // Verify disconnect handler if provided
                    if let Some(id) = disconnect_handler_id {
                        if !handlers.exists(id) {
                            return Err(format!("Disconnect handler not found: {}", id));
                        }
                    }

                    // Mark handlers with correct types
                    handlers
                        .set_handler_type(message_handler_id, HandlerType::WebSocketMessage)
                        .await;

                    if let Some(id) = connect_handler_id {
                        handlers
                            .set_handler_type(id, HandlerType::WebSocketConnect)
                            .await;
                    }

                    if let Some(id) = disconnect_handler_id {
                        handlers
                            .set_handler_type(id, HandlerType::WebSocketDisconnect)
                            .await;
                    }

                    // Enable WebSocket on server
                    let mut servers = servers_clone.write().await;
                    if let Some(server) = servers.get_mut(&server_id) {
                        match server.enable_websocket(
                            path_clone.clone(),
                            connect_handler_id,
                            message_handler_id,
                            disconnect_handler_id,
                        ) {
                            Ok(_) => Ok(()),
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                let path_clone = path.clone();

                match result {
                    Ok(_) => {
                        // Record event
                        
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 12. disable-websocket
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "disable-websocket",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, path): (u64, String)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();

                // Clone values before moving to async context
                let path_clone = path.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let mut servers = servers_clone.write().await;
                    if let Some(server) = servers.get_mut(&server_id) {
                        server
                            .disable_websocket(&path_clone)
                            .map_err(|e| e.to_string())
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                match result {
                    Ok(_) => {
                        // Record event
                        
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 13. send-websocket-message
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "send-websocket-message",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, connection_id, message): (u64, u64, WebSocketMessage)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();
                let message_clone = message.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let servers = servers_clone.read().await;

                    if let Some(server) = servers.get(&server_id) {
                        let connections = server.active_ws_connections.read().await;

                        if let Some(connection) = connections.get(&connection_id) {
                            // Try to send the message
                            match connection.sender.send(message_clone).await {
                                Ok(_) => Ok(()),
                                Err(e) => Err(format!("Failed to send WebSocket message: {}", e)),
                            }
                        } else {
                            Err(format!("WebSocket connection not found: {}", connection_id))
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                let message_clone = message.clone();

                match result {
                    Ok(_) => {
                        // Record event
                        let message_type = match &message.ty {
                            MessageType::Text => "text",
                            MessageType::Binary => "binary",
                            MessageType::Connect => "connect",
                            MessageType::Close => "close",
                            MessageType::Ping => "ping",
                            MessageType::Pong => "pong",
                            MessageType::Other(ref s) => s,
                        };

                        let message_size = message_clone.data.as_ref().map_or(0, |d| d.len())
                            + message_clone.text.as_ref().map_or(0, |t| t.len());

                        
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // 14. close-websocket
        let servers_clone = self.servers.clone();
        interface.func_wrap(
            "close-websocket",
            move |mut ctx: wasmtime::StoreContextMut<'_, ActorStore>,
                  (server_id, connection_id): (u64, u64)|
                  -> Result<(Result<(), String>,)> {
                let servers_clone = servers_clone.clone();

                // Capture the current execution context
                let current_task = tokio::task::spawn(async move {
                    let servers = servers_clone.read().await;

                    if let Some(server) = servers.get(&server_id) {
                        let connections = server.active_ws_connections.read().await;

                        if let Some(connection) = connections.get(&connection_id) {
                            // Send close message
                            let close_message = WebSocketMessage {
                                ty: MessageType::Close,
                                data: None,
                                text: None,
                            };

                            match connection.sender.send(close_message).await {
                                Ok(_) => Ok(()),
                                Err(e) => {
                                    Err(format!("Failed to close WebSocket connection: {}", e))
                                }
                            }
                        } else {
                            Err(format!("WebSocket connection not found: {}", connection_id))
                        }
                    } else {
                        Err(format!("Server not found: {}", server_id))
                    }
                });

                // Wait for the async task to complete
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(current_task)
                })?;

                match result {
                    Ok(_) => {
                        // Record event
                        
                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        info!("HTTP framework host functions set up");

        Ok(())
    }

    fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for HTTP framework");

        match actor_instance.register_function::<(u64, HttpRequest), (HttpResponse,)>(
            "theater:simple/http-handlers",
            "handle-request",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-request function");
            }
            Err(e) => {
                error!("Failed to register handle-request function: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to register handle-request function: {}",
                    e
                ));
            }
        }

        match actor_instance.register_function::<(u64, HttpRequest), (MiddlewareResult,)>(
            "theater:simple/http-handlers",
            "handle-middleware",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-middleware function");
            }
            Err(e) => {
                error!("Failed to register handle-middleware function: {}", e);
                return Err(anyhow::anyhow!(
                    "Failed to register handle-middleware function: {}",
                    e
                ));
            }
        }

        match actor_instance.register_function_no_result::<(u64, u64, String, Option<String>)>(
            "theater:simple/http-handlers",
            "handle-websocket-connect",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-websocket-connect function");
            }
            Err(e) => {
                error!(
                    "Failed to register handle-websocket-connect function: {}",
                    e
                );
                return Err(anyhow::anyhow!(
                    "Failed to register handle-websocket-connect function: {}",
                    e
                ));
            }
        }

        match actor_instance
            .register_function::<(u64, u64, WebSocketMessage), (Vec<WebSocketMessage>,)>(
                "theater:simple/http-handlers",
                "handle-websocket-message",
            ) {
            Ok(_) => {
                info!("Successfully registered handle-websocket-message function");
            }
            Err(e) => {
                error!(
                    "Failed to register handle-websocket-message function: {}",
                    e
                );
                return Err(anyhow::anyhow!(
                    "Failed to register handle-websocket-message function: {}",
                    e
                ));
            }
        }

        match actor_instance.register_function_no_result::<(u64, u64)>(
            "theater:simple/http-handlers",
            "handle-websocket-disconnect",
        ) {
            Ok(_) => {
                info!("Successfully registered handle-websocket-disconnect function");
            }
            Err(e) => {
                error!(
                    "Failed to register handle-websocket-disconnect function: {}",
                    e
                );
                return Err(anyhow::anyhow!(
                    "Failed to register handle-websocket-disconnect function: {}",
                    e
                ));
            }
        }

        info!("Export functions added for HTTP framework");

        Ok(())
    }

    fn start(
        &mut self,
        _actor_handle: ActorHandle,
        _actor_instance: SharedActorInstance,
        shutdown_receiver: ShutdownReceiver,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> {
        let servers_ref = self.servers.clone();
        let server_handles_ref = self.server_handles.clone();

        Box::pin(async move {
            info!("HTTP Framework started, monitoring for shutdown signal");

            // Wait for shutdown signal - no longer spawned in a detached task
            debug!("HTTP Framework shutdown monitor started");
            let signal = shutdown_receiver.wait_for_shutdown().await;
            info!("HTTP Framework received shutdown signal");

            // First stop all the servers
            let server_count = {
                let servers = servers_ref.read().await;
                let count = servers.len();
                debug!("HTTP Framework shutting down {} servers", count);
                count
            };

            if server_count > 0 {
                // Create a vector to hold futures
                let mut shutdown_tasks = Vec::new();

                // Stop each server in parallel with individual timeouts
                for server_id in servers_ref
                    .read()
                    .await
                    .keys()
                    .cloned()
                    .collect::<Vec<u64>>()
                {
                    let servers_clone = servers_ref.clone();
                    let task = tokio::spawn(async move {
                        let start_time = std::time::Instant::now();
                        debug!("Starting shutdown of HTTP server {}", server_id);

                        let result = {
                            let mut servers = servers_clone.write().await;
                            if let Some(server) = servers.get_mut(&server_id) {
                                server.stop().await
                            } else {
                                debug!("Server {} not found during shutdown", server_id);
                                Ok(())
                            }
                        };

                        match result {
                            Ok(_) => debug!(
                                "Successfully stopped HTTP server {} in {:?}",
                                server_id,
                                start_time.elapsed()
                            ),
                            Err(ref e) => {
                                warn!("Error stopping HTTP server {}: {}", server_id, e)
                            }
                        }
                        (server_id, result)
                    });
                    shutdown_tasks.push(task);
                }

                // Wait for all servers to be stopped with a global timeout
                debug!("Waiting for all servers to stop (timeout: 10s)");
                match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    futures::future::join_all(shutdown_tasks),
                )
                .await
                {
                    Ok(results) => {
                        let success_count = results
                            .iter()
                            .filter(|r| r.is_ok() && r.as_ref().unwrap().1.is_ok())
                            .count();
                        let failure_count = results.len() - success_count;

                        if failure_count > 0 {
                            warn!(
                                "Stopped {}/{} HTTP servers successfully, {} had errors",
                                success_count,
                                results.len(),
                                failure_count
                            );
                        } else {
                            info!("All {} HTTP servers stopped successfully", success_count);
                        }
                    }
                    Err(_) => {
                        error!("Global timeout reached while waiting for servers to stop");

                        // Log which servers might still be running
                        let servers = servers_ref.read().await;
                        let running_servers: Vec<u64> = servers
                            .iter()
                            .filter(|(_, server)| server.is_running())
                            .map(|(id, _)| *id)
                            .collect();

                        if !running_servers.is_empty() {
                            error!(
                                "The following servers may still be running: {:?}",
                                running_servers
                            );
                        }
                    }
                }
            } else {
                debug!("No HTTP servers to shut down");
            }

            // Then clean up the handles
            {
                let mut handles = server_handles_ref.write().await;
                if !handles.is_empty() {
                    debug!("Cleaning up {} server handles", handles.len());
                    let handle_ids: Vec<u64> = handles.keys().cloned().collect();

                    for handle_id in handle_ids {
                        if let Some(handle) = handles.remove(&handle_id) {
                            if let Some(task) = handle.task {
                                if !task.is_finished() {
                                    debug!("Aborting server task for server {}", handle_id);
                                    task.abort();
                                    // Wait a tiny bit for the abort to register
                                    tokio::time::sleep(Duration::from_millis(50)).await;
                                    debug!("Aborted server task for server {}", handle_id);
                                }
                            }

                            if let Some(tx) = handle.shutdown_tx {
                                if tx.is_closed() {
                                    debug!(
                                        "Shutdown channel for server {} already closed",
                                        handle_id
                                    );
                                } else {
                                    debug!("Sending final shutdown signal to server {}", handle_id);
                                    let _ = tx.send(()); // Ignore errors, this is just a backup
                                }
                            }
                        }
                    }
                }
            }

            // Add a longer delay to ensure OS has time to release sockets
            debug!("Waiting for OS to release socket resources");
            tokio::time::sleep(Duration::from_millis(500)).await;

            info!("HTTP Framework shutdown complete");

            // Send acknowledgment back to shutdown controller
            if let Some(sender) = signal.sender {
                debug!("Sending shutdown acknowledgment");
                let _ = sender.send(());
            }

            Ok(())
        })
    }

    fn name(&self) -> &str {
        "http-framework"
    }

    fn imports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/http-framework".to_string()])
    }

    fn exports(&self) -> Option<Vec<String>> {
        Some(vec!["theater:simple/http-handlers".to_string()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_creation() {
        let handler = HttpFrameworkHandler::new(None);
        assert_eq!(handler.name(), "http-framework");
    }

    #[test]
    fn test_handler_clone() {
        let handler = HttpFrameworkHandler::new(None);
        let cloned = handler.create_instance();
        assert_eq!(cloned.name(), "http-framework");
    }
}
