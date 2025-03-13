use crate::actor_handle::ActorHandle;
use crate::actor_store::ActorStore;
use crate::events::http::HttpEventData;
use crate::events::{ChainEventData, EventData};
use crate::shutdown::ShutdownReceiver;
use crate::wasm::{ActorComponent, ActorInstance};

use anyhow::Result;

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use thiserror::Error;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::handlers::{HandlerConfig, HandlerRegistry, HandlerType};
use super::server_instance::ServerInstance;
use super::types::*;

#[derive(Error, Debug)]
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

// Helper functions for validation

// Validate hostname or IP address
fn is_valid_host(host: &str) -> bool {
    // Basic validation - could be enhanced with regex for stricter checking
    !host.is_empty() && !host.contains(' ') && host.len() < 255
}

// Validate HTTP method
fn is_valid_method(method: &str) -> bool {
    match method {
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS" | "CONNECT" | "TRACE"
        | "*" => true,
        _ => false,
    }
}

struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    server_id: u64, // Track which server this handle belongs to
}

#[derive(Clone)]
pub struct HttpFramework {
    servers: Arc<RwLock<HashMap<u64, ServerInstance>>>,
    handlers: Arc<RwLock<HandlerRegistry>>,
    next_server_id: Arc<AtomicU64>,
    next_handler_id: Arc<AtomicU64>,
    next_route_id: Arc<AtomicU64>,
    next_middleware_id: Arc<AtomicU64>,
    server_handles: Arc<RwLock<HashMap<u64, ServerHandle>>>,
}

impl HttpFramework {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            next_server_id: Arc::new(AtomicU64::new(1)),
            next_handler_id: Arc::new(AtomicU64::new(1)),
            next_route_id: Arc::new(AtomicU64::new(1)),
            next_middleware_id: Arc::new(AtomicU64::new(1)),
            server_handles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn setup_host_functions(&self, actor_component: &mut ActorComponent) -> Result<()> {
        info!("Setting up HTTP framework host functions");

        let servers_clone = self.servers.clone();
        let handlers_clone = self.handlers.clone();
        let next_server_id = self.next_server_id.clone();
        let next_handler_id = self.next_handler_id.clone();
        let next_route_id = self.next_route_id.clone();
        let next_middleware_id = self.next_middleware_id.clone();
        let server_handles_clone = self.server_handles.clone();

        let mut interface = actor_component
            .linker
            .instance("ntwk:theater/http-framework")
            .expect("could not instantiate http-framework");

        // Create server implementation
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
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "http-framework/create-server".to_string(),
                    data: EventData::Http(HttpEventData::ServerCreate {
                        server_id,
                        host: host.clone(),
                        port,
                        with_tls: config.tls_config.is_some(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Created HTTP server {} on {}:{}",
                        server_id, host, port
                    )),
                });

                // Store server instance
                let servers_clone_inner = servers_clone.clone();
                tokio::spawn(async move {
                    let mut servers = servers_clone_inner.write().await;
                    servers.insert(server_id, server);
                });

                Ok((Ok(server_id),))
            },
        )?;

        // Get server info implementation
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

        // Start server implementation
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
                                // Record event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/start-server".to_string(),
                                    data: EventData::Http(HttpEventData::ServerStart {
                                        server_id,
                                        port,
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Started HTTP server {} on port {}",
                                        server_id, port
                                    )),
                                });

                                // Create server handle for tracking
                                let server_handle = ServerHandle {
                                    shutdown_tx: None, // We don't have this yet
                                    task: None,        // We don't have this yet
                                    server_id,         // Track which server this is for
                                };

                                // Store the handle
                                let handle_clone = server_handles_clone.clone();
                                tokio::spawn(async move {
                                    let mut handles = handle_clone.write().await;
                                    handles.insert(server_id, server_handle);
                                    debug!("Stored handle for server {}", server_id);
                                });

                                Ok((Ok(port),))
                            }
                            Err(e) => {
                                error!("Failed to start server {}: {}", server_id, e);

                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/start-server".to_string(),
                                    data: EventData::Http(HttpEventData::Error {
                                        operation: "start-server".to_string(),
                                        path: format!("server-{}", server_id),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to start HTTP server {}: {}",
                                        server_id, e
                                    )),
                                });

                                Ok((Err(format!("Failed to start server: {}", e)),))
                            }
                        }
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // Stop server implementation
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
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/stop-server".to_string(),
                                    data: EventData::Http(HttpEventData::ServerStop { server_id }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!("Stopped HTTP server {}", server_id)),
                                });

                                Ok((Ok(()),))
                            }
                            Err(e) => {
                                error!("Failed to stop server {}: {}", server_id, e);

                                // Record error event
                                ctx.data_mut().record_event(ChainEventData {
                                    event_type: "http-framework/stop-server".to_string(),
                                    data: EventData::Http(HttpEventData::Error {
                                        operation: "stop-server".to_string(),
                                        path: format!("server-{}", server_id),
                                        message: e.to_string(),
                                    }),
                                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                                    description: Some(format!(
                                        "Failed to stop HTTP server {}: {}",
                                        server_id, e
                                    )),
                                });

                                Ok((Err(format!("Failed to stop server: {}", e)),))
                            }
                        }
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // Destroy server implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/destroy-server".to_string(),
                            data: EventData::Http(HttpEventData::ServerDestroy { server_id }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!("Destroyed HTTP server {}", server_id)),
                        });

                        Ok((Ok(()),))
                    } else {
                        Ok((Err(format!("Server not found: {}", server_id)),))
                    }
                })
            },
        )?;

        // Register handler implementation
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
                    handler_type: HandlerType::Unknown, // Will be determined when used
                };

                // Record event
                ctx.data_mut().record_event(ChainEventData {
                    event_type: "http-framework/register-handler".to_string(),
                    data: EventData::Http(HttpEventData::HandlerRegister {
                        handler_id,
                        name: handler_name.clone(),
                    }),
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    description: Some(format!(
                        "Registered handler {} with name '{}'",
                        handler_id, handler_name
                    )),
                });

                // Store handler
                tokio::spawn(async move {
                    let mut handlers = handlers_clone.write().await;
                    handlers.register(handler_config);
                });

                Ok((Ok(handler_id),))
            },
        )?;

        // Add route implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/add-route".to_string(),
                            data: EventData::Http(HttpEventData::RouteAdd {
                                route_id,
                                server_id,
                                path: path_clone.clone(),
                                method: method_clone.clone(),
                                handler_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Added route {} for {} {} on server {}",
                                route_id, method_clone, path_clone, server_id
                            )),
                        });

                        Ok((Ok(route_id),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Remove route implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/remove-route".to_string(),
                            data: EventData::Http(HttpEventData::RouteRemove {
                                route_id,
                                server_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Removed route {} from server {}",
                                route_id, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Add middleware implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/add-middleware".to_string(),
                            data: EventData::Http(HttpEventData::MiddlewareAdd {
                                middleware_id,
                                server_id,
                                path: path_clone.clone(),
                                handler_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Added middleware {} for path {} on server {}",
                                middleware_id, path_clone, server_id
                            )),
                        });

                        Ok((Ok(middleware_id),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Remove middleware implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/remove-middleware".to_string(),
                            data: EventData::Http(HttpEventData::MiddlewareRemove {
                                middleware_id,
                                server_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Removed middleware {} from server {}",
                                middleware_id, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Enable WebSocket implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/enable-websocket".to_string(),
                            data: EventData::Http(HttpEventData::WebSocketEnable {
                                server_id,
                                path: path_clone.clone(),
                                connect_handler_id,
                                message_handler_id,
                                disconnect_handler_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Enabled WebSocket on path {} for server {}",
                                path_clone, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Disable WebSocket implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/disable-websocket".to_string(),
                            data: EventData::Http(HttpEventData::WebSocketDisable {
                                server_id,
                                path: path.clone(),
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Disabled WebSocket on path {} for server {}",
                                path, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Send WebSocket message implementation
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

                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/send-websocket-message".to_string(),
                            data: EventData::Http(HttpEventData::WebSocketMessage {
                                server_id,
                                connection_id,
                                message_type: message_type.to_string(),
                                message_size,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Sent WebSocket message to connection {} on server {}",
                                connection_id, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        // Close WebSocket connection implementation
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
                        ctx.data_mut().record_event(ChainEventData {
                            event_type: "http-framework/close-websocket".to_string(),
                            data: EventData::Http(HttpEventData::WebSocketDisconnect {
                                server_id,
                                connection_id,
                            }),
                            timestamp: chrono::Utc::now().timestamp_millis() as u64,
                            description: Some(format!(
                                "Closed WebSocket connection {} on server {}",
                                connection_id, server_id
                            )),
                        });

                        Ok((Ok(()),))
                    }
                    Err(e) => Ok((Err(e),)),
                }
            },
        )?;

        info!("HTTP framework host functions set up");

        Ok(())
    }

    pub async fn add_export_functions(&self, actor_instance: &mut ActorInstance) -> Result<()> {
        info!("Adding export functions for HTTP framework");

        actor_instance
            .register_function::<(u64, HttpRequest), (HttpResponse,)>(
                "ntwk:theater/http-handlers",
                "handle-request",
            )
            .expect("Failed to register handle-request function");

        actor_instance
            .register_function::<(u64, HttpRequest), (MiddlewareResult,)>(
                "ntwk:theater/http-handlers",
                "handle-middleware",
            )
            .expect("Failed to register handle-middleware function");

        actor_instance
            .register_function_no_result::<(u64, u64, String, Option<String>)>(
                "ntwk:theater/http-handlers",
                "handle-websocket-connect",
            )
            .expect("Failed to register handle-websocket-connect function");

        actor_instance
            .register_function::<(u64, u64, WebSocketMessage), (Vec<WebSocketMessage>,)>(
                "ntwk:theater/http-handlers",
                "handle-websocket-message",
            )
            .expect("Failed to register handle-websocket-message function");

        actor_instance
            .register_function_no_result::<(u64, u64)>(
                "ntwk:theater/http-handlers",
                "handle-websocket-disconnect",
            )
            .expect("Failed to register handle-websocket-disconnect function");

        info!("Export functions added for HTTP framework");

        Ok(())
    }

    pub async fn start(
        &self,
        _actor_handle: ActorHandle,
        mut shutdown_receiver: ShutdownReceiver,
    ) -> Result<()> {
        // Create task to monitor shutdown signal
        let servers_ref = self.servers.clone();
        let server_handles_ref = self.server_handles.clone();

        info!("HTTP Framework started, monitoring for shutdown signal");

        tokio::spawn(async move {
            debug!("HTTP Framework shutdown monitor started");

            // Wait for shutdown signal
            shutdown_receiver.wait_for_shutdown().await;
            info!("HTTP Framework received shutdown signal");

            // First stop all the servers
            let server_count = {
                let servers = servers_ref.read().await;
                let count = servers.len();
                debug!("HTTP Framework shutting down {} servers", count);

                // Get server IDs
                let server_ids: Vec<u64> = servers.keys().cloned().collect();
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
                            Err(ref e) => warn!("Error stopping HTTP server {}: {}", server_id, e),
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
        });

        Ok(())
    }
}
